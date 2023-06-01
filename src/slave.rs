
enum CommunicationState {
    Init,
    PreOperational,
    SafeOperational,
    Operational,
}

struct Slave<const State: CommunicationState> {
    master: &'a Master,
    rank: u16,
    address: u16,
    mailbox_stuff: ..,
}
impl<const State: CommunicationState> Slave<State> {
    pub fn informations(&self)  {todo!()}
    /// return the current state of the slave
    pub fn state(&self) -> CommunicationState {todo!()}
    /// send a state change request to the slave, and return once the slave has switched
    pub fn switch<const S2: CommunicationState>(self) -> Slave<S2>  {todo!()}
    /// check that the slave is in the desired communication mode, but does not switch it if not
    pub fn expect<const S2: CommunicationState>(self) -> Slave<S2>  {todo!()}
}
impl Slave<Init> {
    pub fn init(&mut self) {
        // setup mailbox
        // check if COE available
        // check if FOE available
    }
    pub fn register_read<T>(&self, field: Field<T>) -> T  {todo!()}
    pub fn register_write<T>(&self, field: Field<T>, value: T) -> T  {todo!()}
}
impl Slave<PreOperational> {
    pub fn sdo_read<T>(&self, sdo: Sdo<T>) -> T   {todo!()}
    pub fn sdo_write<T>(&self, sdo: Sdo<T>, value: T)  {todo!()}
}

