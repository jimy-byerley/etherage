use crate::{
	socket::EthercatSocket,
	rawmaster::RawMaster,
	};


pub struct Master {
    raw: RawMaster<dyn EthercatSocket>,
}
impl Master {
    async fn topology<'a>(&'a self) -> SlaveDiscovery<'a>   {todo!()}
    
    unsafe pub fn get_raw(&self) -> &RawMaster {todo!()}
    pub fn into_raw(self) -> RawMaster {todo!()}
}

