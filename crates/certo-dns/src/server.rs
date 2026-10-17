use std::{net::SocketAddr, time::Duration};

use hickory_server::Server;
use tokio::net::{TcpListener, UdpSocket};
use tracing::info;

use crate::DnsHandler;

const TCP_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_RESPONSE_BUFFER: usize = 64;

/// Errors while binding DNS listeners.
#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    /// Unknown `general.protocol` value.
    #[error("invalid protocol {0:?}")]
    Protocol(String),
    /// The address could not be bound.
    #[error("bind {proto} {addr}: {source}")]
    Bind {
        /// `udp` or `tcp`.
        proto: &'static str,
        /// Requested address.
        addr: String,
        /// Bind failure.
        #[source]
        source: std::io::Error,
    },
}

/// `:53` binds every interface; the family follows the protocol suffix (`4`/`6`).
fn listen_addr(listen: &str, protocol: &str) -> String {
    listen.strip_prefix(':').map_or_else(
        || listen.to_owned(),
        |port| {
            if protocol.ends_with('4') {
                format!("0.0.0.0:{port}")
            } else {
                format!("[::]:{port}")
            }
        },
    )
}

/// Binds the UDP and/or TCP listeners selected by `protocol` and starts serving.
///
/// Returns the running server and the bound addresses.
pub async fn listen(
    listen: &str,
    protocol: &str,
    handler: DnsHandler,
) -> Result<(Server<DnsHandler>, Vec<SocketAddr>), ListenError> {
    let (udp, tcp) = match protocol.trim_end_matches(['4', '6']) {
        "both" => (true, true),
        "udp" => (true, false),
        "tcp" => (false, true),
        _ => return Err(ListenError::Protocol(protocol.to_owned())),
    };
    let addr = listen_addr(listen, protocol);
    let bind_err = |proto, source| ListenError::Bind {
        proto,
        addr: addr.clone(),
        source,
    };
    let mut server = Server::new(handler);
    let mut bound = Vec::new();
    if udp {
        let socket = UdpSocket::bind(&addr)
            .await
            .map_err(|e| bind_err("udp", e))?;
        let local = socket.local_addr().map_err(|e| bind_err("udp", e))?;
        info!(addr = %local, proto = "udp", "starting DNS listener");
        bound.push(local);
        server.register_socket(socket);
    }
    if tcp {
        // Reuse the UDP port when an ephemeral port (:0) was requested.
        let tcp_addr = bound
            .first()
            .map_or_else(|| addr.clone(), ToString::to_string);
        let listener = TcpListener::bind(&tcp_addr)
            .await
            .map_err(|e| bind_err("tcp", e))?;
        let local = listener.local_addr().map_err(|e| bind_err("tcp", e))?;
        info!(addr = %local, proto = "tcp", "starting DNS listener");
        bound.push(local);
        server.register_listener(listener, TCP_TIMEOUT, TCP_RESPONSE_BUFFER);
    }
    Ok((server, bound))
}

#[cfg(test)]
mod tests {
    use super::listen_addr;

    #[test]
    fn wildcard_listen_addresses() {
        assert_eq!(listen_addr(":53", "both"), "[::]:53");
        assert_eq!(listen_addr(":53", "udp4"), "0.0.0.0:53");
        assert_eq!(listen_addr(":53", "tcp6"), "[::]:53");
        assert_eq!(listen_addr("127.0.0.1:53", "udp6"), "127.0.0.1:53");
    }
}
