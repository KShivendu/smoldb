# smoldb

A smol database implemented from scratch. It is heavily inspired by [Qdrant](https://github.com/qdrant/qdrant)'s design.

### Usage:

```
cargo run
```

Now call these APIs at `localhost:9900`

```http
PUT /collections/test
{
  "params": "..."
}

GET /collections/test

PUT /collections/test/points
{
  "points": [ { "id": 0, "payload": { "msg": "hello world" } } ]
}

GET /collections/test/points/0
```

### ToDo:
- [ ] Think about what kind of database this should be.
- [ ] Consensus for config/shard/cluster updates
- [ ] Operations wal and wal delta transfer for data updates

