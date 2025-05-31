# smoldb

A smol database implemented from scratch. It is heavily inspired by [Qdrant](https://github.com/qdrant/qdrant)'s design.


### Usage:

```
cargo run -- --url 127.0.0.1:9900
```

### ToDo:
- [ ] Think about what kind of database this should be.
- [ ] Consensus for config/shard/cluster updates
- [ ] Operations wal and wal delta transfer for data updates

