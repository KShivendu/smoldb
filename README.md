# smoldb 🐣

A smol distributed database implemented from scratch. Parts of it are inspired by [Qdrant](https://github.com/qdrant/qdrant)'s design.

### Usage:

```sh
# Run with Docker
docker pull ghcr.io/kshivendu/smoldb:dev
docker run -p 9000:9000 -v $(pwd)/storage:/storage ghcr.io/kshivendu/smoldb:dev

# Or compile locally:
cargo run -r
```

```bash
# Create collection
curl -X PUT http://localhost:9000/collections/test \
  -H "Content-Type: application/json" \
  -d '{
    "params": "..."
  }'

# Add points
curl -X PUT http://localhost:9000/collections/test/points \
  -H "Content-Type: application/json" \
  -d '{
    "points": [
        { "id": 0, "payload": { "msg": "hello world" } },
        { "id": 10, "payload": { "msg": "foo bar" } },
        { "id": 123, "payload": { "msg": "bar baz" } }
     ]
  }'

# Get all points (response below)
curl -X GET http://localhost:9000/collections/test/points

{
    "result": {
        "points": [
            {
                "id": 0,
                "payload": {
                    "msg": "hello world"
                }
            },
            # ...
        ]
    },
    "time": 0.000211024
}

# Get specific point
curl -X GET http://localhost:9000/collections/test/points/0

# Get collection's cluster info (response below)
curl -X GET http://localhost:9000/collections/test/cluster

# Response:
{
    "peer_id": 0,
    "shard_count": 2,
    "local_shards": [
        {
            "shard_id": 1,
            "point_count": 1,
            "state": "Active"
        },
        {
            "shard_id": 0,
            "point_count": 2,
            "state": "Active"
        }
    ],
    "remote_shards": []
}
```

Check [roadmap](./ROADMAP.md) for details
