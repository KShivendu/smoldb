# smoldb

A smol database implemented from scratch. It is heavily inspired by [Qdrant](https://github.com/qdrant/qdrant)'s design.

### Usage:

```
cargo run
```

Now call these APIs at `localhost:9900`

```bash
# Create collection
curl -X PUT http://localhost:6333/collections/test \
  -H "Content-Type: application/json" \
  -d '{
    "params": "..."
  }'

# Add points
curl -X PUT http://localhost:6333/collections/test/points \
  -H "Content-Type: application/json" \
  -d '{
    "points": [ { "id": 0, "payload": { "msg": "hello world" } } ]
  }'

# Get point (response below)
curl -X GET http://localhost:6333/collections/test/points/0

# Response:
{
  "id": 0,
  "payload": {
    "msg": "hello world"
  }
}
```

### ToDo:
- [ ] Think about what kind of database this should be.
- [ ] Consensus for config/shard/cluster updates
- [ ] Operations wal and wal delta transfer for data updates

