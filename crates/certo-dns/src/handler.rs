use hickory_server::{
    net::runtime::Time,
    proto::{
        op::{Edns, Header, HeaderCounts, MessageType, Metadata, OpCode, ResponseCode},
        rr::Record,
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    zone_handler::MessageResponseBuilder,
};
use tracing::error;

use crate::{ChallengeResponder, DynamicCatalog};

/// Top-level DNS request handler.
///
/// Names under the base domain, and names outside every user zone, are answered by the
/// [`ChallengeResponder`]; names inside a user zone are answered by the zone catalog.
#[derive(Debug, Clone)]
pub struct DnsHandler {
    challenge: ChallengeResponder,
    zones: DynamicCatalog,
}

impl DnsHandler {
    /// Combines the challenge responder with the user zone catalog.
    pub const fn new(challenge: ChallengeResponder, zones: DynamicCatalog) -> Self {
        Self { challenge, zones }
    }

    fn uses_zone_catalog(&self, request: &Request) -> bool {
        let Some(query) = request.queries.queries().first() else {
            return false;
        };
        !self.challenge.owns(query.name()) && self.zones.covers(query.name())
    }

    async fn answer_challenge<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let mut metadata = Metadata::response_from_request(&request.metadata);
        let mut edns = None;
        let mut answers: Vec<Record> = Vec::new();
        let mut authority: Vec<Record> = Vec::new();

        let accepts_query = request.metadata.message_type == MessageType::Query
            && request.metadata.op_code == OpCode::Query;
        let mut process = accepts_query;
        if let Some(req_edns) = &request.edns {
            let mut resp = Edns::new();
            resp.set_max_payload(512);
            if req_edns.version() != 0 {
                resp.set_rcode_high(ResponseCode::BADVERS.high());
                metadata.response_code = ResponseCode::BADVERS;
                process = false;
            }
            edns = Some(resp);
        }

        if process {
            for query in request.queries.queries() {
                let answer = self.challenge.answer(query).await;
                metadata.authoritative |= answer.authoritative;
                metadata.response_code = answer.rcode;
                answers.extend(answer.records);
            }
            if metadata.authoritative && metadata.response_code == ResponseCode::NXDomain {
                authority.extend(self.challenge.soa().cloned());
            }
        }

        let response = MessageResponseBuilder::new(&request.queries, edns.as_ref()).build(
            metadata,
            answers.iter(),
            authority.iter(),
            std::iter::empty(),
            std::iter::empty(),
        );
        match response_handle.send_response(response).await {
            Ok(info) => info,
            Err(e) => {
                error!(error = %e, "failed to send response");
                let mut metadata = Metadata::response_from_request(&request.metadata);
                metadata.response_code = ResponseCode::ServFail;
                Header {
                    metadata,
                    counts: HeaderCounts::default(),
                }
                .into()
            }
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for DnsHandler {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        if self.uses_zone_catalog(request) {
            self.zones
                .handle_request::<R, T>(request, response_handle)
                .await
        } else {
            self.answer_challenge(request, response_handle).await
        }
    }
}
