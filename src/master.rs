use crate::{
	socket::EthercatSocket,
	rawmaster::RawMaster,
	};


pub struct Master {
    raw: RawMaster,
}
impl Master {
//     pub async fn topology<'a>(&'a self) -> SlaveDiscovery<'a>   {todo!()}
    
    pub unsafe fn get_raw(&self) -> &RawMaster {todo!()}
    pub fn into_raw(self) -> RawMaster {todo!()}
}

