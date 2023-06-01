
struct Master {
    raw: RawMaster<dyn EthercatSocket>,
}
impl Master {
    async fn topology<'a>(&'a self) -> SlaveDiscovery<'a>   {todo!()}
}

