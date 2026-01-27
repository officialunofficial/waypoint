# Contributing

## Terminology

- **Snapchain Events** - Farcaster protocol consensus events (called "OnChainEvents" in code)
- **On-chain Events** - Actual Ethereum transactions (verifications, signers, etc.)

## Setup

```bash
git clone https://github.com/officialunofficial/waypoint.git
cd waypoint
make env-setup
# If running local Postgres and Redis instances
docker compose --profile services up
# Run database migrations (requires psql)
scripts/run-migrations.sh
make build
make test
```

## Workflow

1. Fork and create a branch
2. Make changes
3. Run `make test` and `make fmt`
4. Submit PR with conventional commit messages (`feat:`, `fix:`, etc.)

## Code Style

- Follow Rust idioms
- Use rustfmt (config in rustfmt.toml)
- Document public APIs with rustdoc
- Keep functions focused
