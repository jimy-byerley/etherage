# Etherage

This crate aims to bring yet an other implementation of an Ethercat master, The [Ethercat communication protocol](https://en.wikipedia.org/wiki/EtherCAT) is a network protocol working on top of the [Ethernet](https://en.wikipedia.org/wiki/Ethernet) layer, designed for realtime industrial applications (like robotics). It is standardized by [ETG (Ethercat Technology Group)](https://www.ethercat.org/default.htm)

[![Crates.io](https://img.shields.io/crates/v/etherage.svg)](https://crates.io/crates/etherage)
[![Docs.rs](https://docs.rs/etherage/badge.svg)](https://docs.rs/etherage)

<img src="logo/etherage.svg" width=300/>

## goals

- feature complete

  This library is designed to allow everything said possible in the [ethercat specifications](https://www.ethercat.org/en/downloads/downloads_A02E436C7A97479F9261FDFA8A6D71E5.htm). No restriction *shall* be added by this implementation.

- modularity

  The user can decide what will be initialized or not on the master and each slave, and can dive deeply in the details the communication at the same time as performing higher level operations.

- close to the ethercat intrinsics

  We believe the best way to produce bloatfull code is trying to make a circle fit a square, so this library should be transparent on how the Ethercat protocol really works. Ethercat is a standard after all.

- generic master implementation

  This implementation *shall* be ready for any purpose.

- maximum performance, reliability, flexibility

  This master implementation *shall* be fast enough for realtime operations even running on poor hardware, and shall be reliable for industrial use. It is better to have it initialize fast as well.

- protocol-safety

  rust *memory safety* prevents any unexpected error due to a bad usage of the memory through proposed tools. Here we define *protocol safety* to prevent unexpected communication error due to a bad usage of this library. It means the hereby proposed API makes it impossible for the master to break the communication without writing *unsafe* code.

- ease of use

  of course this crate must be nice to use

## non-goals

- no-std  *(at the moment)*
- adapt to vendor-specific implementations of ethercat, or to vendor-specific devices
- make abstraction of what the Ethercat protocol really does
- fit the OSI model

##   Current complete feature list

- [x] master over different sockets
    + [x] raw ethernet
    + [x] UDP
- [ ] minimalistic features
    - [x] PDU commands
    - [x] access to logical & physical memories
    - [ ] slave information access
- [x] mailbox
    + generic messaging
    + [ ] forwarding
    + [x] COE
        - [x] SDO read/write
        - [ ] PDO read/write
        - [ ] informations
        - [x] tools for mapping
    + [ ] EOE
    + [ ] FOE
- [ ] distributed clock
    + [ ] static drift
    + [ ] dynamic drift
- convenience
  - [x] logical memory & slave group management tools
  - [x] mapping tools
- optimization features
    + [x] multiple PDUs per ethercat frame (speed up and compress transmissions)
    + [x] tasks for different slaves or for same slave are parallelized whenever possible
    + [x] no dynamic allocation in transmission and realtime functions
    + [x] async API and implementation to avoid threads context switches

## getting started

### requirements

- if connecting to an ethercat segment with a direct ethernet connection (the common practice), you need permissions to open a raw-socket (usually *root* access)
- if connecting to an ethercat segment through a UDP socket, any normal user can proceed.
- no special OS dependency or configuration is needed, only what is in [`Cargo.toml`](Cargo.toml)

### take the path

The best way to take a tour of what `etherage` can do is to look at the [examples](examples)

First: check that the example takes the right network interface (default is `eno1`) in the main of the desired example.

Then compile and run an example, like listing connected slaves:

```shell
cargo build --example slaves_discovery
sudo target/debug/examples/slaves_discovery
```

typical output with 8 Omron servodrives:

```
  slave 7: "R88D-1SN01H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.00" software "V1.02.00"
  slave 0: "R88D-1SN02H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.00" software "V1.02.00"
  slave 6: "R88D-1SN02H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.01" software "V1.04.00"
  slave 3: "R88D-1SN02H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.00" software "V1.02.00"
  slave 5: "R88D-1SN02H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.00" software "V1.02.00"
  slave 4: "R88D-1SN02H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.00" software "V1.02.00"
  slave 1: "R88D-1SN02H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.00" software "V1.02.00"
  slave 2: "R88D-1SN04H-ECT" - ecat type 17 rev 0 build 3 - hardware "V1.01" software "V1.04.00"
```

