# Etherage

This crate aims to bring yet a other implementation of an Ethercat master, The [Ethercat communication protocol](https://en.wikipedia.org/wiki/EtherCAT) is a network protocol working on top of the [Ethernet](https://en.wikipedia.org/wiki/Ethernet) layer, designed for realtime industrial applications (like robotics). It is standardized by [ETG (Ethercat Technology Group)](https://www.ethercat.org/default.htm)

**NOTE:** At the moment, this crate is in its early development.

## goals

- feature complete

  This library is designed to allow everything said possible in the [ethercat specifications](https://www.ethercat.org/en/downloads/downloads_A02E436C7A97479F9261FDFA8A6D71E5.htm). No restriction *shall* be added by this implementation.

- modularity

  The user can decide what will be initialized or not on the master and each slave, and can dive deeply in the details the communication at the same time as performing higher level operations.

- close to the ethercat intrinsics

  We believe the best way to produce bloatfull code is trying to make a circle fit a square, so this library should be transparent on how the Ethercat protocol really works. Ethercat is a standard after all.

- generic master implementation

  This implementation *shall* be ready for any use.

- maximum performance, reliability, flexibility

  This master implementation *shall* be fast enough for realtime operations even running on poor hardware, and shall be reliable for industrial use. It is better to have it initialize fast as well.

- protocol-safety

  rust *memory safety* prevents any unexpected error due to a bad usage of the memory through proposed tools. Here we define *protocol safety* to prevent unexpected communication error due to a bad usage of this library. It means the hereby proposed API makes it impossible for the master to break the communication without writing *unsafe* code.

- ease of use

  of course this crate must be nice to use

## non-goals

- no-std  *(atm)*
- adapt to vendor-specific implementations of ethercat, or to vendor-specific devices
- make abstraction of what the Ethercat protocol really does

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
- no special OS dependency or configuration is needed, only what is `Cargo.toml`

### take the path

The best way to take a tour of what `etherage` can do is to look at the [examples](examples)

First: check that the example takes the right network interface in the desired example `fn main`: by default it is `eno1`. Then compile and run:

```shell
cargo build --example slaves_discovery
sudo target/debug/examples/slaves_discovery
```

