
## Running benchmarks:

```bash
cargo bench -- --list # List benches
cargo bench # Run all benches
cargo bench upserts # Run all benches in upserts group

# Access the reports at `target/criterion/report/index.html`
python -m http.server .

# For flame graph:
cargo flamegraph --bench upsert -o flamegraph.svg -- --bench
```
