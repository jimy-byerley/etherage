# Ethercotic - Ethercat dark reference

This document is a summary of points not covered in the ethercat specifications, and aims to help as reference for unsaid points that are hard to understand refering only on the official specifications.
Everything here is discovered empirically in the world of *Etherca Inconita*, reported here in the manuscriptum of Ethercotic.

## data representation in network frames

## ethercat frame types

an ethercat frame in an ethernet or UDP frame starts with a header mentioning its type

ethercat frame types:

- PDU 

  the only type used with slaves

- Mailbox

  not used with a slave, but used by other ethernet devices to send mailbox requests to the slaves through the master, the master must support it, and listen for mailbox external requests on the ethernet

- Network variables

  no usage known

## mailbox

- must be configured in pre-operational communication state (AL state)
- must use a 2 sync channel, each in 1-buffer mode
- mailbox content can have a variable size (depending on the content proper header), but must be set as a whole. It is allowed to read/write only parts, but only when all the buffer is read/written, the message will be considered send/received by the master, and only then the slave will proceed with responses.
- the sync channels of the mailbox can use any buffer in the physical memory, any free space can be used.

## mapping

- sync channel buffers can be at any location with any size
- the channels areas shall not overlap or interlace.
- a channel's area consist of 3 successive swaping buffers.
  - the first one only shall be accessed by the FMMU, mapped values must hence be computed in the first.
  - the area for the 2 others shall directly follow the first buffer
- buffers shall by word-aligned
