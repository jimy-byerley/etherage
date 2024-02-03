# Synchronization

## Sdo synchronizarion manager

Register description

- sync_type: Define synchronization type
    - 0x00: Free Run (not synchronized)
    - 0x01: SM-Synchronous — synchronized With SM Event
    - 0x02: DC Sync0 — synchronized with Sync0 Event
    - 0x03: DC Sync1 — synchronized with Sync1 Event
- cycle_time:
    - Free Run (Synchronization Type 0x00): Time between two local timer events in ns
    - Synchronous with SM2 (Synchronization Type 0x01): Minimum time between two SM2 events in ns
    - DC Sync0 (Synchronization Type 0x02): Sync0 Cycle Time (Register 0x9A3-0x9A0) ns
- shift_time: Time between related event and the associated action (Outputs valid hardware) in ns. Shift of output valid equal or greater than 0x1C32:09
- supported_sync_type: Detailled synchronization capacity
    - Bit 0: Free Run supported
    - Bit : SM Synchronous supported
    - Bit 4:2 : DC Type supported:
        - 000 No DC
        - 001 DC sync0
        - 010 DC sync1
        - 100 Application with fixed Sync0
    - Bit 6:5: Shift Settings
        - 00 Output Shift supported
        - 01 Output Shift with local timer (Shift Time)
        - 10 Output Shift with Sync1
    - Bit 9…6: Reserved for future use
    - Bit 10: Delay Times should be measured (because they
depend on the configuration)
    - Bit 11: Delay Tine is fix (synchronization is done by
hardware)
    - Bit 13...11: Reserved for future use
    - Bit 14: Dynamic Cycle Times
        - Times described in 0x1C32 are variable (depend
the actual configuration) This is used for e.g. EtherCAT gateway devices. The slave shall support cycle time measurement in OP State. The cycle time measurement is started by writing I to 0x1 C32:08. If this bit is set, the default values Of the times to be measured (Minimum Cycle Time, Calc And Copy Time, Delay Time) could be 0. The default values could be set in INIT and PREOP State. Should only be Set, if the slave cannot calculate the cycle time after receiving all Start-up SDO in transition PS.
    - Bit 15: Reserved for future use
- min_cycle_time: Minimum cycle time supported by the slave (maximum duration time of the local cycle) in ns. It might be necessary to Start the Dynamic Cycle Time measurement SI04, Bit 14 and SI08, Bit 0 to get a valid value used in Synchronous Or DC Mode
- calc_copy_time: Time needed by the application controller to copy the process data from the Sync manager to the local memory and perform calculations necessary before the data is sent to the process. Minimum time for Outputs to SYNC-Event. Used in DC mode.
- min_delay_time: Only important for DC Sync0/1 (Synchronization type 0x02 or 0x03): Minimum Hardware delay time of the slave. Because of software synchronization there could be a distance between the minimum and the maximum delay time. Distance between minimum and maximum delay time (SI 9) shall be smaller than 1 µS. If SI 4, Bit 11 is 1, fris Sublndex contains the value of SI 9
- get_cycle_time:
    - Bit 0:
        - 0: Measurement of local cycle time stopped
        - 1: Measurement of local cycle time started.
    - If Written again, the measured values are reset
    - Used in Synchronous or (DC Mode with variable Cycle Time)
    - Bit 1:
        - 0: --
        - 1 B10:B11: Reset the error counters
        - Bit 2..15: reserved for future use
- delay_time: Only important for DC Sync0/1 (Synchronization type = 0x02 or 0x03): Hardware delay time Of the slave. Time from receiving the trigger (Sync0 or Sync1 Event) to drive output values to the time until they become valid in the process (eg electrical signal available). if Subindex 7 Minimum Delay Time is supported and unequal 0, this Delay Time is the Maximum Delay Time
- sync0_cycle_time: Only important for DC Sync0 (Synchronization type 0x03) and subordinated local cycles Time between two Sync0 signals if fixed Sync0 Cycle Time is needed by the application
- sm_evt_cnt This error counter is incremented When the application expects a SM event but does not receive it in time and as consequence the data cannot be copied any more.
used in DC Mode
- cycle_time_evt_smal: This error counter is incremented when the Cycle time is too small So that the local cycle cannot be completed and input data cannot be provided before next SM event. Used in Synchronous Or DC Mode.
- shift_time_evt_small: This error counter is incremented When the time distance between the trigger (Sync0) and the Outputs Valid is too short because of a too short Shift Time or Sync1 Cycle Time. Used in DC Mode.
- toggle_failure_cnt: This error counter is incremented When the slave Supports the RxPDO Toggle and does not receive new RxPDO data from the master (RxPDO Toggle set to TRUE)
- min_cycle_dist: Minimum Cycle Distance in ns. Used in conjunction with SI 16 to monitor the little between two SM-events.
- max_cycle_dist:	Maximum cycle distance in ns used in conjunction with SI 15 to monitor the littlle between two SM-events
- min_sm_sync_dist:	Minimum SM SYNC Distance in ns used in coniunction with SI 18 to monitor the jitter the SM-event and SYNCO-event in DCSYNC0-Mode
- max_sm_sync_dist:	Maximum SM SYNC Distance in ns used in coniunction with SI 17 to monitor the jitter between the SM-event and SYNCO-event in DCSYNC0-Mode
- sync_error: Shall be supported if SWEvent Missed Counter (SI11) or Cycle Time Too Small Counter (SI12) or Shift Time too Short Counter (SI13) is supported. Mappable in TXPDO
    - 0: Synchronization Error or Sync Error not supported
    - 1: Synchronization Error

Access type ordered by sub index

| Var                  |    | Free |SM 2/3 | SM 2/3 Shift | DC  | DC shift | DC shift SYNC 1 | DC SYNC1 | DC subordinate|
-----------------------|----|------|-------|--------------|-----|----------|-----------------|----------|---------------|
| sync_type            |  1 | R/RW | RW    | RW           | R/RW| RW       | RW              | R/RW     | RW            |
| cycle_time           |  2 | R/RW | R/RW  | R/RW         | R   | R        | R               | R        | R             |
| shift_time           |  3 | 	   |       |              |	    | RW       |                 |          | RW            |
| supported_sync_type  |  4 | R    | R	   | R	          | R   | R	       | R	             | R	    | R             |
| min_cycle_time       |  5 | R    | R	   | R	          | R   | R	       | R	             | R	    | R             |
| calc_copy_time       |  6 |      | 	   | 	          | R   | R	       | R	             | R	    | R             |
| get_cycle_time       |  7 |      | 	   | 	          |     | 	       | 	 	         |          |               |
| delay_time           |  8 |      | RW	   | RW	          | RW  | RW	   | RW	             |RW	    | RW            |
| sync0_cycle_time     | 10 |      | 	   | 	          |     | 	       | R               |          |               |
| sm_evt_cnt           | 11 |      | R	   | R	          | R   | R	       | R	             | R	    | R             |
| cycle_time_evt_small | 12 |      | R	   | R	          | R   | R	       | R	             | R	    | R             |
| shift_time_evt_small | 13 |      | 	   | 	          | R   | R	       | R	             | R	    | R             |
| toggle_failure_cnt   | 14 |      | R	   | R	          | R   | R	       | R	             | R	    | R             |
| min_cycle_dist       | 15 |      | R	   | R	          | R   | R	       | R	             | R	    | R             |
| max_cycle_dist       | 16 |      | R	   | R	          | R   | R	       | R	             | R	    | R             |
| min_sm_sync_dist     | 17 |      | 	   | 	          | R   | R	       | R	             | R	    | R             |
| max_sm_sync_dist     | 18 |      | 	   | 	          | R   | R	       | R	             | R	    | R             |
| sync_error           | 32 |      | R	   | R	          | R   | R	       | R	             |R	        | R             |

| Var                  |    | Free |SM 2/3 | SM 2/3 Shift | DC  | DC shift | DC shift SYNC 1 | DC SYNC1 | DC subordinate|
|----------------------|----|------|-------|--------------|-----|----------|-----------------|----------|---------------|
| sync_type	           |  1 | C    | M     | M            | M   | M        | M               | M        | M             |
| cycle_time	       |  2 | O    | O     | O            | O   | M        | M               | O        | M             |
| supported_sync_type  |  4 | C    | M     | M            | M   | M        | M               | M        | M             |
| min_cycle_time	   |  5 | C    | M     | M            | M   | M        | M               | M        | M             |
| calc_copy_time	   |  6 | -    | -     | -            | M   | M        | M               | M        | M             |
| min_delay_time	   |  7 | -    | -     | -            | -   | -        | -               | -        | -             |
| get_cycle_time	   |  8 | -    | C     | C            | C   | C        | C               | C        | C             |
| delay_time	       |  9 | -    | -     | -            | M - | M -      | M               | M -      | M             |
| sync0_cycle_time	   | 10 | -    | -     | -            | -   | -        | -               | -        | M             |
| sm_evt_cnt	       | 11 | -    | O     | O            | O   | O        | O               | O        | O             |
| cycle_time_evt_small | 12 | -    | M     | M            | M   | M        | M               | M        | M             |
| shift_time_evt_small | 13 | -    | -     | -            | O   | O        | O               | O -      | O             |
| toggle_failure_cnt   | 14 | -    | O     | O            | O   | O        | O               | O        | O             |
| min_cycle_dist	   | 15 | -    | O -   | O -          | O - | O -      | O               | O -      | O             |
| max_cycle_dist	   | 16 | -    | O -   | O -          | O - | O -      | O               | O -      | O             |
| min_sm_sync_dist	   | 17 | -    | -     | -            | O - | O -      | O               | O -      | O             |
| max_sm_sync_dist	   | 18 | -    | -     | -            | O - | O -      | O               | O -      | O             |
| sync_error	       | 32 | -    | C     | C            | C   | C        | C               | C        | C             |

### Register for synchronize input valid data



### Register for synchronize outpute valid data