use async_trait::async_trait;
use tokio::sync::mpsc;
use crate::proto::OCNPacket;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn start_server(&self, addr: &str) -> anyhow::Result<mpsc::Receiver<(OCNPacket, Box<dyn ConnectionHandle>)>>;
    async fn connect(&self, addr: &str) -> anyhow::Result<Box<dyn ConnectionHandle>>;
}

#[async_trait]
pub trait ConnectionHandle: Send + Sync {
    async fn send(&mut self, packet: &OCNPacket) -> anyhow::Result<()>;
    async fn recv(&mut self) -> anyhow::Result<Option<OCNPacket>>;
    fn remote_addr(&self) -> std::net::SocketAddr;
}
