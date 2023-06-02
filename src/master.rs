use crate::{
	socket::EthercatSocket,
	rawmaster::RawMaster,
	};


pub struct Master {
    raw: RawMaster<impl EthercatSocket>,
}
impl Master {
//     pub async fn topology<'a>(&'a self) -> SlaveDiscovery<'a>   {todo!()}
    
    pub unsafe fn get_raw(&self) -> &RawMaster<impl EthercatSocket> {todo!()}
    pub fn into_raw<S: EthercatSocket>(self) -> RawMaster<S> {todo!()}
}

