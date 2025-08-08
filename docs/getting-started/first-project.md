# Your First Bake Project

This tutorial walks you through creating a complete Bake project from scratch, covering core concepts and common patterns you'll use in real projects.

## What You'll Build

We'll create a full-stack web application project with:
- Frontend (React app)
- Backend (Node.js API) 
- Shared utilities
- Testing, building, and deployment recipes
- Caching and dependency management

## Prerequisites

- Bake installed ([Installation Guide](installation.md))
- Node.js and npm installed
- Basic understanding of JavaScript/TypeScript

## Project Setup

### 1. Create Project Structure

```bash
mkdir fullstack-app
cd fullstack-app

# Create the project structure
mkdir -p frontend/src frontend/test
mkdir -p backend/src backend/test  
mkdir -p shared/src shared/test
mkdir -p deployment/scripts
```

### 2. Project Configuration

Create the main project configuration:

```yaml
# bake.yml
name: "Fullstack Web Application"
description: "Example project showing Bake features"

variables:
  # Environment-specific variables
  environment: development
  node_version: "18"
  
  # Build configuration
  version: "1.0.0"
  build_mode: debug
  
  # Deployment settings
  deploy_target: staging
  base_url: "https://api-{{var.deploy_target}}.example.com"

# Cookbooks are automatically discovered from cookbook.yml files
config:
  max_parallel: 6
  fast_fail: true
  cache:
    local:
      enabled: true
      path: .bake/cache
  minVersion: "0.11.0"
```

## Creating Cookbooks

### 3. Shared Utilities Cookbook

Start with shared utilities that other cookbooks depend on:

```yaml
# shared/cookbook.yml
name: shared
description: "Shared utilities and types"

variables:
  package_name: "@app/shared"
  output_dir: "dist"

recipes:
  install:
    description: "Install shared dependencies"
    cache:
      inputs:
        - "package.json"
        - "package-lock.json"
    run: |
      echo "Installing {{var.package_name}} dependencies..."
      npm ci

  build:
    description: "Build shared utilities"
    cache:
      inputs:
        - "src/**/*.ts"
        - "tsconfig.json"
        - "package.json"
      outputs:
        - "{{var.output_dir}}/**/*"
    dependencies:
      - install
    run: |
      echo "Building {{var.package_name}}..."
      npm run build

  test:
    description: "Run shared utility tests"
    cache:
      inputs:
        - "src/**/*.ts"
        - "test/**/*.ts"
        - "jest.config.js"
      outputs:
        - "coverage/**/*"
    dependencies:
      - build
    run: |
      echo "Testing {{var.package_name}}..."
      npm test

  lint:
    description: "Lint shared code"
    cache:
      inputs:
        - "src/**/*.ts"
        - "test/**/*.ts"
        - ".eslintrc.js"
    run: |
      npm run lint
```

### 4. Frontend Cookbook

```yaml
# frontend/cookbook.yml
name: frontend
description: "React frontend application"

variables:
  app_name: "frontend-app"
  build_env: "{{var.environment}}"
  dist_dir: "dist-{{var.build_env}}"
  api_url: "{{var.base_url}}/api"

recipes:
  install:
    description: "Install frontend dependencies"
    cache:
      inputs:
        - "package.json"
        - "package-lock.json"
    run: |
      echo "Installing frontend dependencies..."
      npm ci

  build:
    description: "Build React application for {{var.build_env}}"
    cache:
      inputs:
        - "src/**/*"
        - "public/**/*"
        - "package.json"
        - "tsconfig.json"
        - "vite.config.ts"
      outputs:
        - "{{var.dist_dir}}/**/*"
    dependencies:
      - install
      - shared:build
    environment:
      - NODE_ENV
      - VITE_API_URL
    variables:
      VITE_API_URL: "{{var.api_url}}"
      NODE_ENV: "{{var.build_env}}"
    run: |
      echo "Building {{var.app_name}} for {{var.build_env}}..."
      echo "API URL: {{var.api_url}}"
      npm run build
      echo "Build output in {{var.dist_dir}}"

  test:
    description: "Run frontend tests"
    cache:
      inputs:
        - "src/**/*.{ts,tsx}"
        - "test/**/*.{ts,tsx}"
        - "package.json"
        - "jest.config.js"
      outputs:
        - "coverage/**/*"
        - "test-results.xml"
    dependencies:
      - shared:test
    run: |
      echo "Running frontend tests..."
      npm test -- --coverage --watchAll=false

  e2e:
    description: "Run end-to-end tests"
    cache:
      inputs:
        - "e2e/**/*.spec.ts"
        - "playwright.config.ts"
    dependencies:
      - build
      - backend:start-test
    run: |
      echo "Running E2E tests..."
      npm run test:e2e

  preview:
    description: "Start preview server"
    dependencies:
      - build
    run: |
      echo "Starting preview server..."
      npm run preview
```

### 5. Backend Cookbook

```yaml  
# backend/cookbook.yml
name: backend
description: "Node.js API server"

variables:
  service_name: "api-server"
  port: 3001
  db_url: "postgresql://localhost:5432/app_{{var.environment}}"

recipes:
  install:
    description: "Install backend dependencies"
    cache:
      inputs:
        - "package.json"
        - "package-lock.json"
    run: |
      npm ci

  build:
    description: "Build API server"
    cache:
      inputs:
        - "src/**/*.ts"
        - "tsconfig.json"
        - "package.json"
      outputs:
        - "dist/**/*"
    dependencies:
      - install
      - shared:build
    run: |
      echo "Building {{var.service_name}}..."
      npm run build

  test:
    description: "Run backend tests"
    cache:
      inputs:
        - "src/**/*.ts"
        - "test/**/*.ts"
        - "jest.config.js"
      outputs:
        - "coverage/**/*"
    dependencies:
      - build
    run: |
      npm test -- --coverage

  start-test:
    description: "Start test server for E2E tests"
    dependencies:
      - build
    environment:
      - DATABASE_URL
      - PORT
    variables:
      DATABASE_URL: "{{var.db_url}}"
      PORT: "{{var.port}}"
    run: |
      echo "Starting test server on port {{var.port}}..."
      npm run start:test &
      sleep 5  # Wait for server to start

  migrate:
    description: "Run database migrations"
    cache:
      inputs:
        - "migrations/**/*.sql"
        - "migrate.js"
    environment:
      - DATABASE_URL
    variables:
      DATABASE_URL: "{{var.db_url}}"
    run: |
      echo "Running migrations for {{var.environment}}..."
      npm run migrate
```

### 6. Deployment Cookbook

```yaml
# deployment/cookbook.yml  
name: deployment
description: "Deployment and infrastructure"

variables:
  target_env: "{{var.deploy_target}}"
  version_tag: "v{{var.version}}"

recipes:
  build-images:
    description: "Build Docker images"
    cache:
      inputs:
        - "Dockerfile"
        - "docker-compose.yml"
    dependencies:
      - frontend:build
      - backend:build
    run: |
      echo "Building Docker images for {{var.target_env}}..."
      docker build -t app-frontend:{{var.version_tag}} .
      docker build -t app-backend:{{var.version_tag}} -f backend/Dockerfile .

  deploy-staging:
    description: "Deploy to staging environment"
    dependencies:
      - build-images
      - backend:migrate
    run: |
      echo "Deploying {{var.version_tag}} to staging..."
      ./scripts/deploy-staging.sh

  deploy-production:
    description: "Deploy to production environment"
    dependencies:
      - frontend:e2e
      - backend:test
      - build-images
    variables:
      target_env: production
    run: |
      echo "Deploying {{var.version_tag}} to production..."
      echo "⚠️  Production deployment requires manual approval"
      ./scripts/deploy-production.sh

  smoke-test:
    description: "Run smoke tests after deployment"
    dependencies:
      - deploy-staging
    run: |
      echo "Running smoke tests against {{var.target_env}}..."
      curl -f {{var.base_url}}/health
      echo "✅ Smoke tests passed"
```

## Create Sample Files

### 7. Add Sample Package Files

```bash
# Create sample package.json files
cat > shared/package.json << 'EOF'
{
  "name": "@app/shared",
  "version": "1.0.0",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc",
    "test": "jest",
    "lint": "eslint src/"
  }
}
EOF

cat > frontend/package.json << 'EOF'  
{
  "name": "frontend-app",
  "version": "1.0.0",
  "scripts": {
    "build": "vite build",
    "test": "jest",
    "test:e2e": "playwright test",
    "preview": "vite preview"
  }
}
EOF

cat > backend/package.json << 'EOF'
{
  "name": "api-server", 
  "version": "1.0.0",
  "scripts": {
    "build": "tsc",
    "test": "jest",
    "start:test": "node dist/server.js",
    "migrate": "node migrate.js"
  }
}
EOF
```

### 8. Add Sample Source Files

```bash
# Shared utilities
cat > shared/src/index.ts << 'EOF'
export interface User {
  id: number;
  name: string;
  email: string;
}

export const formatUser = (user: User): string => {
  return `${user.name} (${user.email})`;
};
EOF

# Frontend component
mkdir -p frontend/src/components
cat > frontend/src/App.tsx << 'EOF'
import React from 'react';
import { User, formatUser } from '@app/shared';

const App: React.FC = () => {
  const user: User = { id: 1, name: 'John Doe', email: 'john@example.com' };
  
  return (
    <div>
      <h1>Hello Bake!</h1>
      <p>User: {formatUser(user)}</p>
    </div>
  );
};

export default App;
EOF

# Backend API
cat > backend/src/server.ts << 'EOF'
import express from 'express';
import { User, formatUser } from '@app/shared';

const app = express();
const port = process.env.PORT || 3001;

app.get('/health', (req, res) => {
  res.json({ status: 'healthy', timestamp: new Date().toISOString() });
});

app.get('/users', (req, res) => {
  const users: User[] = [
    { id: 1, name: 'John Doe', email: 'john@example.com' },
    { id: 2, name: 'Jane Smith', email: 'jane@example.com' }
  ];
  
  res.json(users.map(formatUser));
});

app.listen(port, () => {
  console.log(`Server running on port ${port}`);
});
EOF

# Sample tests
cat > shared/test/index.test.ts << 'EOF'
import { formatUser } from '../src';

test('formatUser formats correctly', () => {
  const user = { id: 1, name: 'John', email: 'john@test.com' };
  expect(formatUser(user)).toBe('John (john@test.com)');
});
EOF
```

## Running Your Project

### 9. Explore Bake Commands

```bash
# See all available recipes
bake --list-recipes

# Show execution plan
bake --show-plan

# Build everything
bake

# Build specific components
bake frontend:build backend:build

# Run tests across all cookbooks  
bake :test

# Build for production
bake --var environment=production --var build_mode=release

# Deploy to staging
bake deployment:deploy-staging

# Debug configuration
bake --render
bake frontend:build --render --var environment=production
```

### 10. Understanding the Output

When you run `bake`, you'll see:
- Dependency resolution and execution order
- Parallel execution of independent recipes
- Cache hits/misses based on input changes
- Clear progress reporting

Try modifying source files and running again to see caching in action.

## Advanced Patterns

### Environment-Specific Overrides

```yaml
# In bake.yml, add environment overrides
variables:
  environment: development
  
overrides:
  production:
    environment: production
    build_mode: release
    deploy_target: production
    max_parallel: 12
  
  staging:
    environment: staging
    deploy_target: staging
```

### Recipe Templates

Create reusable patterns:

```bash
mkdir -p .bake/templates
```

```yaml
# .bake/templates/node-service.yml
name: "node-service"
description: "Standard Node.js service template"

parameters:
  service_name:
    type: string
    required: true
  port:
    type: number
    default: 3000

template:
  description: "{{params.service_name}} service"
  dependencies: ["install", "shared:build"]
  environment: ["PORT", "NODE_ENV"]
  variables:
    PORT: "{{params.port}}"
  run: |
    echo "Starting {{params.service_name}} on port {{params.port}}"
    npm start
```

Use in cookbooks:

```yaml  
recipes:
  start:
    template: node-service
    params:
      service_name: "API Server"
      port: 3001
```

## What You've Learned

✅ **Project Organization** - Structure projects with cookbooks and recipes  
✅ **Automatic Discovery** - Cookbooks are found automatically by scanning for `cookbook.yml` files  
✅ **Dependency Management** - Express dependencies between recipes  
✅ **Variable System** - Use templated variables across configurations  
✅ **Caching Strategy** - Define inputs/outputs for intelligent caching  
✅ **Parallel Execution** - Run independent recipes simultaneously  
✅ **Environment Configuration** - Handle multiple deployment targets  
✅ **Recipe Templates** - Create reusable patterns  

## Next Steps

- **[Configuration Guide](../guides/configuration.md)** - Learn all configuration options
- **[Variables Guide](../guides/variables.md)** - Master the variable system  
- **[Caching Guide](../guides/caching.md)** - Optimize build performance
- **[Recipe Templates](../guides/recipe-templates.md)** - Advanced template patterns
- **[Best Practices](../guides/best-practices.md)** - Production-ready patterns

## Troubleshooting

**Recipes not running in parallel?**
- Check for unnecessary dependencies
- Increase `max_parallel` in config

**Cache not working?**
- Verify `inputs` and `outputs` are correct
- Check file paths are relative to cookbook root

**Variable substitution issues?**
- Use `bake --render` to debug resolved configuration
- Check variable scoping (project → cookbook → recipe)

**Dependencies not found?**
- Verify cookbook names in dependencies
- Use `cookbook:recipe` format for cross-cookbook dependencies