//! Shared helpers for socket-level DNS tests.
#![allow(dead_code, unreachable_pub, reason = "each test binary uses a subset")]

use std::{net::SocketAddr, str::FromStr, time::Duration};

use hickory_server::{
    Server,
    proto::{
        op::{Edns, Message, MessageType, OpCode, Query, ResponseCode},
        rr::{Name, RData, Record, RecordType},
    },
    server::RequestHandler,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
};

pub fn name(s: &str) -> Name {
    Name::from_str(s).unwrap()
}

pub fn rr(owner: &str, rtype: RecordType, rdata: &str) -> Record {
    Record::from_rdata(name(owner), 300, RData::try_from_str(rtype, rdata).unwrap())
}

pub async fn start(handler: impl RequestHandler) -> SocketAddr {
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = udp.local_addr().unwrap();
    let tcp = TcpListener::bind(addr).await.unwrap();
    let mut server = Server::new(handler);
    server.register_socket(udp);
    server.register_listener(tcp, Duration::from_secs(5), 64);
    tokio::spawn(async move { server.block_until_done().await });
    addr
}

pub fn query(qname: &str, rtype: RecordType, dnssec_ok: bool) -> Message {
    let mut msg = Message::new(4242, MessageType::Query, OpCode::Query);
    msg.add_query(Query::query(name(qname), rtype));
    if dnssec_ok {
        let mut edns = Edns::new();
        edns.set_dnssec_ok(true).set_max_payload(4096);
        msg.set_edns(edns);
    }
    msg
}

pub async fn udp(addr: SocketAddr, msg: &Message) -> Message {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.send_to(&msg.to_vec().unwrap(), addr).await.unwrap();
    let mut buf = vec![0u8; 65535];
    let n = tokio::time::timeout(Duration::from_secs(2), sock.recv(&mut buf))
        .await
        .unwrap()
        .unwrap();
    Message::from_vec(&buf[..n]).unwrap()
}

/// Sends one TCP request and reads responses until the transfer's closing SOA
/// (or the first response for anything that is not a successful AXFR).
pub async fn tcp(addr: SocketAddr, msg: &Message) -> Vec<Message> {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let bytes = msg.to_vec().unwrap();
    stream
        .write_all(&u16::try_from(bytes.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    stream.write_all(&bytes).await.unwrap();

    let mut out = Vec::new();
    let mut soas = 0;
    loop {
        let mut len = [0u8; 2];
        tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut len))
            .await
            .unwrap()
            .unwrap();
        let mut buf = vec![0u8; usize::from(u16::from_be_bytes(len))];
        stream.read_exact(&mut buf).await.unwrap();
        let resp = Message::from_vec(&buf).unwrap();
        soas += resp
            .answers
            .iter()
            .filter(|r| r.record_type() == RecordType::SOA)
            .count();
        let done = resp.metadata.response_code != ResponseCode::NoError || soas >= 2;
        out.push(resp);
        if done {
            return out;
        }
    }
}

pub fn types(records: &[Record]) -> Vec<RecordType> {
    records.iter().map(Record::record_type).collect()
}
