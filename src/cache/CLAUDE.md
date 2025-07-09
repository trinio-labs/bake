# Cache Module - CLAUDE.md

This file provides guidance for working with the cache module in the bake project.

## Module Overview

The cache module implements a multi-tier caching system that allows recipes to skip execution when their inputs
haven't changed. It supports multiple cache backends including local filesystem, AWS S3, and Google Cloud Storage.

## Key Files

- **mod.rs** - Main cache module with traits and common functionality
- **builder.rs** - Cache strategy composition and configuration
- **local.rs** - Local filesystem cache implementation
- **s3.rs** - AWS S3 remote cache implementation
- **gcs.rs** - Google Cloud Storage cache implementation

## Architecture

### Cache Strategy Pattern

The cache module uses the Strategy pattern where:

- `CacheStrategy` trait defines the interface for all cache implementations
- `CacheBuilder` composes multiple cache strategies into a hierarchical system
- Cache operations try strategies in order until a hit is found

### Cache Key Generation

Cache keys are computed from:

- Input file hashes (from `project/hashing.rs`)
- Recipe dependency hashes
- Recipe command content and configuration
- Environment variables that affect execution

### Cache Tiers

1. **Local Cache** - Fastest, stored in `.bake/cache/` directory
2. **S3 Cache** - Shared across team/CI, requires AWS credentials
3. **GCS Cache** - Alternative cloud storage, requires GCP credentials

## Key Concepts

### Cache Operations

- **Get**: Check if cached result exists for given key
- **Put**: Store recipe execution result in cache
- **Cache Hit**: Recipe can skip execution, outputs are restored
- **Cache Miss**: Recipe must be executed, results cached for future

### Cache Invalidation

Cache entries become invalid when:

- Input files change (different hash)
- Recipe command or configuration changes
- Dependencies change (transitive invalidation)
- Manual cache clearing (`bake clean`)

## Implementation Guidelines

### Error Handling

- Cache operations should be non-blocking for recipe execution
- Cache failures should log warnings but not fail the build
- Network cache operations have timeouts and retry logic

### Performance Considerations

- Cache operations are async and use structured concurrency
- Local cache is checked first (fastest)
- Remote cache operations happen in parallel when possible
- Cache compression reduces storage and transfer costs

### Testing

- Use `TestCacheStrategy` for predictable cache behavior in tests
- Test cache hit/miss scenarios
- Test cache invalidation on input changes
- Test error handling for cache failures

## Configuration

Cache behavior is configured in `bake.yml`:

```yaml
cache:
  local: true
  s3:
    bucket: "my-cache-bucket"
    region: "us-east-1"
  gcs:
    bucket: "my-gcs-cache"
    project: "my-project"
```

## Development Tips

- Cache keys must be deterministic across machines
- Use structured logging for cache operations
- Cache operations should be idempotent
- Consider cache eviction policies for local cache
- Test cache behavior with different file systems and permissions

## Common Patterns

### Adding New Cache Backend

1. Implement the `CacheStrategy` trait
2. Add configuration options to `CacheBuilder`
3. Add tests for the new backend
4. Update documentation

### Cache Debugging

- Use verbose mode (`-v`) to see cache operations
- Check cache directories for stored artifacts
- Verify cache key generation is consistent
- Monitor cache hit rates in logs
