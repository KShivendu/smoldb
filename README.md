# smoldb

A smol distributed database implemented from scratch. It is heavily inspired by [Qdrant](https://github.com/qdrant/qdrant)'s design. It's mainly built with the goal of learning.

### Usage:

```
cargo run
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
```

Check [roadmap](./ROADMAP.md) for details
