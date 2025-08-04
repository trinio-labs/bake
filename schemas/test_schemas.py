#!/usr/bin/env python3
"""
Test script to validate bake configuration files against JSON schemas.
"""

import json
import yaml
import os
from pathlib import Path
from jsonschema import validate, ValidationError

def load_schema(schema_path):
    """Load a JSON schema from file."""
    with open(schema_path, 'r') as f:
        return json.load(f)

def load_yaml_file(yaml_path):
    """Load a YAML file and convert to dict."""
    with open(yaml_path, 'r') as f:
        return yaml.safe_load(f)

def test_schema_validation(schema_path, yaml_path, description):
    """Test validation of a YAML file against a schema."""
    print(f"\n=== {description} ===")
    try:
        schema = load_schema(schema_path)
        data = load_yaml_file(yaml_path)
        
        validate(instance=data, schema=schema)
        print(f"✅ {yaml_path} is valid against {schema_path}")
        return True
        
    except ValidationError as e:
        print(f"❌ {yaml_path} validation failed:")
        print(f"   Error: {e.message}")
        print(f"   Path: {' -> '.join(str(p) for p in e.absolute_path)}")
        return False
    except FileNotFoundError as e:
        print(f"⚠️  File not found: {e}")
        return False
    except Exception as e:
        print(f"❌ Unexpected error: {e}")
        return False

def main():
    """Run all schema validation tests."""
    schemas_dir = Path(__file__).parent
    project_root = schemas_dir.parent
    test_resources = project_root / "resources" / "tests" / "valid"
    
    print("Bake Configuration Schema Validation Tests")
    print("=" * 50)
    
    results = []
    
    # Test project configuration schema
    results.append(test_schema_validation(
        schemas_dir / "bake-project.schema.json",
        test_resources / "bake.yml",
        "Project Configuration (bake.yml)"
    ))
    
    # Test cookbook schema
    results.append(test_schema_validation(
        schemas_dir / "cookbook.schema.json",
        test_resources / "foo" / "cookbook.yml",
        "Cookbook Configuration (foo/cookbook.yml)"
    ))
    
    # Test template schema
    results.append(test_schema_validation(
        schemas_dir / "recipe-template.schema.json",
        test_resources / ".bake" / "templates" / "build-template.yml",
        "Recipe Template (build-template.yml)"
    ))
    
    results.append(test_schema_validation(
        schemas_dir / "recipe-template.schema.json",
        test_resources / ".bake" / "templates" / "test-template.yml",
        "Recipe Template (test-template.yml)"
    ))
    
    # Summary
    print("\n" + "=" * 50)
    print("SUMMARY")
    print("=" * 50)
    passed = sum(results)
    total = len(results)
    print(f"Passed: {passed}/{total}")
    
    if passed == total:
        print("🎉 All schema validations passed!")
        return 0
    else:
        print("⚠️  Some validations failed. Please check the schemas.")
        return 1

if __name__ == "__main__":
    exit(main())