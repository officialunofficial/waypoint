# Contributing to Waypoint

Thank you for your interest in contributing to Waypoint! This document provides guidelines and instructions for contributing.

## Note on Terminology

When working with this codebase, it's important to understand the distinction between different types of events:

- **Snapchain Events**: These are events from Farcaster's Snapchain protocol. In the codebase and database schema, they're often referred to as "OnChainEvents" following Farcaster's protocol terminology, but they're not actual blockchain transactions. They're consensus events from Snapchain's internal block structure.

- **True On-chain Events**: These refer to actual blockchain transactions like verifications, signers, etc. that occur on the Ethereum blockchain.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## How to Contribute

### Reporting Bugs

If you find a bug, please create an issue with the following information:

- A clear, descriptive title
- Steps to reproduce the issue
- Expected behavior
- Actual behavior
- Any relevant logs or error messages
- Your environment (OS, Rust version, Docker version, etc.)

### Suggesting Features

We welcome feature suggestions! Please create an issue with:

- A clear, descriptive title
- A detailed description of the proposed feature
- Any relevant background or context
- Potential implementation approaches, if you have ideas

### Pull Requests

1. Fork the repository
2. Create a new branch for your changes
3. Make your changes
4. Run tests and ensure they pass
5. Update documentation if necessary
6. Submit a pull request

#### Pull Request Process

1. Update the README.md or documentation with details of changes if appropriate
2. Update the sample configurations if your changes affect configuration
3. Maintain code style consistency with the rest of the project
4. Write or update tests for the changes you've made
5. Make sure your code passes all tests and linting
6. Use conventional commit messages for your changes (e.g., "feat: add new feature", "fix: resolve issue")

## Development Setup

1. Install prerequisites:
   - Rust 1.85+
   - PostgreSQL 17+
   - Redis
   - Protobuf compiler

2. Clone the repository:
   ```bash
   git clone https://github.com/unofficialrun/waypoint.git
   cd waypoint
   ```

3. Set up your environment:
   ```bash
   cp config/sample.toml config.toml
   # Edit config.toml with your settings
   ```

4. Build the project:
   ```bash
   cargo build
   ```

5. Run tests:
   ```bash
   cargo test
   ```

## Code Style

- Follow Rust idioms and conventions
- Use the included rustfmt.toml configuration
- Document public functions and types with rustdoc comments
- Keep functions small and focused on a single responsibility
- Prefer descriptive function and variable names

