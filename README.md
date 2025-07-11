# smoldb

A smol distributed database implemented from scratch. It is heavily inspired by [Qdrant](https://github.com/qdrant/qdrant)'s design.

### Usage:

```sh
# Run with Docker
docker pull smoldot/smoldb

# Or compile locally:
cargo run -r
```

```bash
# Create collection
curl -X PUT http://localhost:9900/collections/test \
  -H "Content-Type: application/json" \
  -d '{
    "params": "..."
  }'

# Add points
curl -X PUT http://localhost:9900/collections/test/points \
  -H "Content-Type: application/json" \
  -d '{
    "points": [ { "id": 0, "payload": { "msg": "hello world" } } ]
  }'

# Get point (response below)
curl -X GET http://localhost:9900/collections/test/points/0

# Response:
{
  "id": 0,
  "payload": {
    "msg": "hello world"
  }
}

# Get collection's cluster info (response below)
curl -X GET http://localhost:9900/collections/test/cluster

# Response:
{
    "peer_id": 0,
    "shard_count": 2,
    "local_shards": [
        {
            "shard_id": 1,
            "point_count": 0,
            "state": "Active"
        },
        {
            "shard_id": 0,
            "point_count": 1,
            "state": "Active"
        }
    ],
    "remote_shards": []
}
```

Check [roadmap](./ROADMAP.md) for details
