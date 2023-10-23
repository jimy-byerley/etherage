#[cfg(test)]
mod tests {
    use std::{io::Error, sync::Arc};
    use etherage::{EthercatError, registers::AlError, can::CanError};
    use bilge::prelude::*;
    use thiserror::Error;

    #[bitsize(4)]
    #[derive(Debug, Error)]
    enum Machin{
        #[error("Error A")]
        A = 0x01,
        #[error("Error B")]
        B = 0x02,
    }



    #[test]
    fn display_ethercaterr_fmt() {
        let arc = Arc::new(Error::new(std::io::ErrorKind::InvalidInput, "File nout found"));
        let err_io: EthercatError<Arc<Error>> = EthercatError::Io(arc.clone());
        let err_master: EthercatError<&'static str> = EthercatError::Master("Master not found");
        let err_proto: EthercatError<&'static str> = EthercatError::Protocol("Protocol incompatible");
        let err_timer: EthercatError<&'static str> = EthercatError::Timeout("DC timeout");


        dbg!(err_io);
        dbg!(err_master);
        dbg!(err_proto);
        dbg!(err_timer);
    }

    #[test]
    fn test_compatibility_bilge_anyhow(){
        println!("{}", Machin::A);
        println!("{:?}", Machin::A);
        println!("{}", Machin::B);
        println!("{:?}", Machin::B);
    }
}