## Milestones:

- Single node:
    - [x] Persist data locally - used [sled kv](https://github.com/spacejam/sled)
    - [x] Shards with hashring
    - [ ] Shard level WAL for durability and faster writes?
    - [ ] What should smoldb optimize for? Vectors, text, logs, columns, rows, in-memory operation, etc?
    - [ ] RAM, Mmap, Disk (s3?) read/writes

- Distributed deployment:
    - [x] Introduce APIs for inter-node (p2p) communication
    - [x] Read/write from/to remote shards
    - [ ] Working consensus for syncing collection/shard state using Raft + p2p gRPC APIs
    - [ ] Sync missed writes to other replicas
