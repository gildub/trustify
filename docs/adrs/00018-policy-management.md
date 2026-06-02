# 00018. Policy Management

Date: 2026-06-02

## Status

PROPOSED

## Context

Trustify provides SBOM storage, analysis, and vulnerability tracking but lacks automated policy enforcement. Organizations need to validate SBOMs against security and compliance policies (licensing, vulnerabilities, provenance) without relying on manual, inconsistent review processes.

Policy management is the prerequisite step: Trustify must be able to store, retrieve, and manage policy references before any policy validation can be triggered. Policy references identify external policy sources (e.g. git repositories) that a validation engine (such as Conforma) fetches at runtime — Trustify does not store the policy content itself.

This ADR covers policy management (CRUD) which provides the building block for future integration with the Conforma validation engine.

### Requirements

Users need the ability to:

1. Define and manage multiple policy configurations
2. Associate policies with validation backends (initially Conforma)

## Decision

Add a `policy` module to Trustify that stores references to external policies and exposes a CRUD API for managing them.

Trustify stores only the identity and location of a policy (id, name, URL/ref, configuration). Policy content is not cached — the validation engine fetches it at runtime from the referenced source.

### The Data Model

**`policy`** — stores references to external policies, not the policies themselves

- `id` (UUID, PK)
- `name` (VARCHAR, unique) — user-friendly label
- `description` (TEXT) — what this policy enforces
- `policy_type` (ENUM) — `'Conforma'`
- `configuration` (JSONB) — see model below
- `revision` (UUID) — used for conditional UPDATE (optimistic concurrency via `ETag`)

**`policy.configuration` JSONB model:**

| Field                  | Type     | Required        | Description                                                                                           |
| ---------------------- | -------- | --------------- | ----------------------------------------------------------------------------------------------------- |
| `policy_ref`           | string   | yes             | Policy source URL, e.g. `"git://[URL]?ref=[BRANCH OR TAG]"`                                           |
| `auth`                 | object   | no              | Credentials for private repos; sensitive values encrypted via `ring::aead` AES-256-GCM (never logged) |
| `auth.type`            | string   | yes (if `auth`) | `"token"`, `"ssh_key"`, or `"none"`                                                                   |
| `auth.token_encrypted` | string   | no              | AES-256-GCM encrypted bearer/PAT token, prefixed with encryption scheme                               |
| `policy_paths`         | string[] | no              | Sub-paths within the repo to evaluate                                                                 |
| `exclude`              | string[] | no              | Rule codes to skip during validation                                                                  |
| `include`              | string[] | no              | If non-empty, only these rule codes are evaluated                                                     |
| `timeout_seconds`      | integer  | no              | Per-policy override of the default execution timeout                                                  |

`policy.configuration` example:

```json
{
  "policy_ref": "git://github.com/org/policy-repo?ref=main",
  "auth": {
    "type": "token",
    "token_encrypted": "AES-256-GCM:<base64-nonce>:<base64-ciphertext>"
  },
  "policy_paths": ["policy/lib", "policy/release"],
  "exclude": ["hello_world.minimal_packages"],
  "include": [],
  "timeout_seconds": 300
}
```

#### Data Model Implementation

```rust
enum ValidatorKind {
    Null,
    Conforma,
}
```

```rust
/// The policy reference information
#[derive(Serialize, Deserialize)]
struct Policy {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    policy_type: ValidatorKind,
    configuration: serde_json::Value,
    /// Conditional updates compare this revision (also exposed as `ETag` on GET).
    revision: Uuid,
}
```

```rust
/// Policy information that can be mutated
#[derive(Serialize, Deserialize)]
struct PolicyRequest {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    policy_type: ValidatorKind,
    configuration: serde_json::Value,
}
```

```rust
/// Credentials for private policy repos (`policy.configuration.auth`)
#[derive(Serialize, Deserialize)]
struct PolicyAuth {
    /// `"token"`, `"ssh_key"`, or `"none"`
    #[serde(rename = "type")]
    auth_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token_encrypted: Option<String>,
}
```

```rust
/// Policy configuration (stored as JSONB)
#[derive(Serialize, Deserialize)]
struct PolicyConfiguration {
    policy_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth: Option<PolicyAuth>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    policy_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u32>,
}
```

## Consequences

### Policy Management

Trustify stores only external references and does not cache policy content. When `policy.policy_type` is `"Conforma"`, the validation engine fetches the policy at validation time from the git source specified in `policy.configuration.policy_ref`.

The trade-off:

- Validation always uses the latest policy content from the referenced branch or tag, but network failures or policy repo outages will cause execution errors.
- For private policy repositories, authentication credentials are stored in the `configuration` JSONB column and encrypted at rest using `ring::aead` (AES-256-GCM authenticated encryption); they are never logged The `ring` crate is already a direct dependency of the project (used for digest hashing), so no new dependency is required.

## Trustify API Endpoints

```
POST   /api/v2/policy          # Create a new policy reference
GET    /api/v2/policy          # List policy references
GET    /api/v2/policy/{id}     # Get a single policy reference
PUT    /api/v2/policy/{id}     # Update a policy reference
DELETE /api/v2/policy/{id}     # Delete a policy reference
```

### Permissions

The policy module introduces the following permissions, following the existing Trustify CRUD convention:

| Permission      | Description                |
| --------------- | -------------------------- |
| `create.policy` | Create policy references   |
| `read.policy`   | List/get policy references |
| `update.policy` | Update policy references   |
| `delete.policy` | Delete policy references   |

These permissions map to the default OIDC scope groups:

| Scope             | Permissions granted |
| ----------------- | ------------------- |
| `create:document` | `create.policy`     |
| `read:document`   | `read.policy`       |
| `update:document` | `update.policy`     |
| `delete:document` | `delete.policy`     |

Endpoint permission requirements:

| Endpoint                     | Permission      |
| ---------------------------- | --------------- |
| `POST /api/v2/policy`        | `create.policy` |
| `GET /api/v2/policy`         | `read.policy`   |
| `GET /api/v2/policy/{id}`    | `read.policy`   |
| `PUT /api/v2/policy/{id}`    | `update.policy` |
| `DELETE /api/v2/policy/{id}` | `delete.policy` |

### POST `/api/v2/policy`

Create a new policy reference.

#### Request

| part | name | type            | description |
| ---- | ---- | --------------- | ----------- |
| body | -    | `PolicyRequest` |             |

#### Response

- 201 - the policy was created

  ```yaml
  id: <id> # ID of the created policy
  ```

  And:

  ```
  Location: /api/v2/policy/<id>
  ```

- 400 - if the request could not be understood
- 401 - if the user was not authenticated
- 403 - if the user was authenticated but not authorized
- 409 - if a policy with the same name already exists

### GET `/api/v2/policy`

List policy references, optionally filtered.

By default, the entries will be sorted by name ascending.

#### Request

| part  | name     | type       | description                                             |
| ----- | -------- | ---------- | ------------------------------------------------------- |
| query | `q`      | "q" string | "q style" query string                                  |
| query | `limit`  | u64        | Maximum number of items to return                       |
| query | `offset` | u64        | Initial items to skip before actually returning results |

The following `q` parameters are supported:

- `name`: Filters policies by their name.

#### Response

- 200 - if the user is allowed to read policies

  ```rust
  #[derive(Serialize, Deserialize)]
  struct PaginatedPolicy {
      total: u64,
      items: Vec<Policy>,
  }
  ```

- 401 - if the user was not authenticated
- 403 - if the user was authenticated but not authorized

### GET `/api/v2/policy/{id}`

Get a single policy reference by ID.

#### Request

| part | name | type     | description             |
| ---- | ---- | -------- | ----------------------- |
| path | `id` | `String` | ID of the policy to get |

#### Response

- 200 - if the policy was found

  | part    | name   | type     | description                        |
  | ------- | ------ | -------- | ---------------------------------- |
  | body    | -      | `Policy` | The policy information             |
  | headers | `ETag` | string   | Value which indicates the revision |

- 401 - if the user was not authenticated
- 404 - if the policy was not found or the user doesn't have permission to read this policy

### PUT `/api/v2/policy/{id}`

Update an existing policy reference.

#### Request

| part   | name      | type             | description                    |
| ------ | --------- | ---------------- | ------------------------------ |
| path   | `id`      | `String`         | ID of the policy to update     |
| header | `IfMatch` | `Option<String>` | ETag value, revision to update |
| body   | -         | `PolicyRequest`  | The new content                |

#### Response

- 204 - the policy was updated
- 400 - if the request could not be understood
- 401 - if the user was not authenticated
- 403 - if the user was authenticated but not authorized
- 404 - if the policy was not found
- 409 - if a policy with the same name already exists
- 412 - if the `IfMatch` header was present, but its value didn't match the stored revision

### DELETE `/api/v2/policy/{id}`

Delete an existing policy reference.

Deleting a policy will fail if there are validation results referencing it.

#### Request

| part   | name      | type             | description                    |
| ------ | --------- | ---------------- | ------------------------------ |
| path   | `id`      | `String`         | ID of the policy to delete     |
| header | `IfMatch` | `Option<String>` | ETag value, revision to delete |

#### Response

- 204 - if the policy was successfully deleted
- 204 - if the policy was already deleted
- 400 - if the request could not be understood
- 401 - if the user was not authenticated
- 403 - if the user was authenticated but not authorized
- 409 - if the policy has associated validation results
- 412 - if the `IfMatch` header was present, but its value didn't match the stored revision

## File Structure

```
modules/policy/
├── Cargo.toml
└── src/
    ├── error.rs                # Error types
    ├── lib.rs
    ├── endpoints/
    │   └── mod.rs              # REST endpoints
    ├── model/
    │   ├── mod.rs
    │   └── policy.rs           # Policy API models
    └── service/
        ├── mod.rs
        └── policy_manager.rs   # Policy configuration management
```
