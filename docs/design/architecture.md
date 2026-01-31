# Trustify Architecture

This document presents Trustify's architecture using the C4 model (Context, Container, Component, Code).

## Level 1: System Context

Shows how Trustify fits into the broader security and supply chain ecosystem.

```mermaid
C4Context
    title System Context - Trustify SBOM and Security Advisory Platform

    Person(securityAnalyst, "Security Analyst", "Reviews vulnerabilities and manages security advisories")
    Person(developer, "Developer", "Queries package vulnerabilities and SBOM data")
    Person(complianceOfficer, "Compliance Officer", "Ensures software license and security compliance")

    System(trustify, "Trustify", "SBOM and Security Advisory Management Platform - ingests, stores, and analyzes software bills of materials and security advisories")

    System_Ext(sbomSources, "SBOM Sources", "CycloneDX, SPDX documents from builds, registries")
    System_Ext(advisorySources, "Advisory Sources", "CSAF, OSV, CVE feeds from vendors, security organizations")
    System_Ext(weaknessSources, "CWE Database", "Common Weakness Enumeration data")
    SystemDb_Ext(oidcProvider, "OIDC Provider", "Keycloak, Auth0, etc - authentication and authorization")
    System_Ext(storageBackend, "Object Storage", "S3, MinIO - stores original documents")

    Rel(developer, trustify, "Queries vulnerabilities and dependencies", "HTTPS/REST")
    Rel(securityAnalyst, trustify, "Uploads and analyzes security data", "HTTPS/REST")
    Rel(complianceOfficer, trustify, "Generates compliance reports", "HTTPS/REST")

    Rel(trustify, sbomSources, "Imports SBOMs", "HTTPS")
    Rel(trustify, advisorySources, "Imports advisories", "HTTPS")
    Rel(trustify, weaknessSources, "Imports CWE data", "HTTPS")
    Rel(trustify, oidcProvider, "Authenticates users", "OIDC")
    Rel(trustify, storageBackend, "Stores/retrieves documents", "S3 API")

    UpdateRelStyle(developer, trustify, $offsetY="-50")
    UpdateRelStyle(securityAnalyst, trustify, $offsetX="-40")
    UpdateRelStyle(complianceOfficer, trustify, $offsetY="-50")
```

## Level 2: Container Diagram

Shows the high-level technology and architectural components of Trustify.

```mermaid
C4Container
    title Container Diagram - Trustify Platform

    Person(user, "User", "Security analyst, developer, or compliance officer")
    System_Ext(oidc, "OIDC Provider", "Authentication")
    System_Ext(externalSources, "External Sources", "SBOM/Advisory feeds")
    SystemDb_Ext(objectStorage, "Object Storage", "S3/MinIO")

    Container_Boundary(trustify, "Trustify Platform") {
        Container(webUI, "Web UI", "React/TypeScript", "Provides web interface for browsing SBOMs, vulnerabilities, and advisories")
        Container(apiServer, "API Server", "Rust/Actix-web", "REST API endpoints for all operations")
        Container(importerService, "Importer Service", "Rust/Tokio", "Scheduled fetching of SBOMs and advisories from external sources")
        Container(ingestorService, "Ingestor Service", "Rust", "Parses and stores SBOM/advisory documents into database")
        Container(analysisService, "Analysis Service", "Rust/Petgraph", "Builds and queries dependency DAG, vulnerability analysis")
        ContainerDb(postgres, "PostgreSQL Database", "PostgreSQL 17", "Stores entities: advisories, vulnerabilities, SBOMs, packages, relationships")
        Container(graphqlAPI, "GraphQL API", "Rust/async-graphql", "Alternative query interface")
    }

    Rel(user, webUI, "Uses", "HTTPS")
    Rel(user, apiServer, "Uses", "HTTPS/REST")
    Rel(webUI, apiServer, "Calls", "JSON/HTTPS")
    Rel(user, oidc, "Authenticates", "OIDC")
    Rel(apiServer, oidc, "Validates tokens", "OIDC")

    Rel(importerService, externalSources, "Fetches documents", "HTTPS")
    Rel(importerService, ingestorService, "Sends documents")
    Rel(ingestorService, objectStorage, "Stores original docs", "S3 API")
    Rel(ingestorService, postgres, "Writes parsed data", "SQL")

    Rel(apiServer, ingestorService, "Upload documents")
    Rel(apiServer, analysisService, "Query analysis")
    Rel(apiServer, postgres, "Reads", "SQL")
    Rel(analysisService, postgres, "Reads graph data", "SQL")
    Rel(graphqlAPI, postgres, "Reads", "SQL")

    UpdateRelStyle(user, webUI, $offsetY="-40")
    UpdateRelStyle(webUI, apiServer, $offsetY="-40")
    UpdateRelStyle(importerService, ingestorService, $offsetX="-50")
```

## Level 3: Component Diagram - API Server

Shows the internal components and services within the API Server.

```mermaid
C4Component
    title Component Diagram - API Server (Trustify Core)

    Container(webUI, "Web UI", "React", "User interface")
    Container(ingestor, "Ingestor", "Rust", "Document ingestion")
    Container(analysis, "Analysis Service", "Rust", "Graph analysis")
    ContainerDb(db, "PostgreSQL", "Database", "Persistent storage")

    Container_Boundary(apiServer, "API Server") {
        Component(authMiddleware, "Auth Middleware", "Actix-web middleware", "Validates OIDC tokens and enforces authorization")

        Component(advisoryEndpoints, "Advisory Endpoints", "Actix-web handlers", "CRUD for security advisories")
        Component(sbomEndpoints, "SBOM Endpoints", "Actix-web handlers", "CRUD for SBOMs")
        Component(purlEndpoints, "PURL Endpoints", "Actix-web handlers", "Query packages by PURL")
        Component(vulnEndpoints, "Vulnerability Endpoints", "Actix-web handlers", "Query CVEs and vulnerabilities")
        Component(productEndpoints, "Product Endpoints", "Actix-web handlers", "Manage products and versions")
        Component(analysisEndpoints, "Analysis Endpoints", "Actix-web handlers", "Component dependency traversal")

        Component(advisoryService, "Advisory Service", "Business logic", "Advisory operations and queries")
        Component(sbomService, "SBOM Service", "Business logic", "SBOM operations and queries")
        Component(purlService, "PURL Service", "Business logic", "Package URL operations")
        Component(vulnService, "Vulnerability Service", "Business logic", "Vulnerability lookups")
        Component(productService, "Product Service", "Business logic", "Product management")

        Component(queryEngine, "Query Engine", "TrustifyQuery", "Flexible query DSL parser and executor")
        Component(graphService, "Graph Service", "SeaORM", "Database access layer")
    }

    Rel(webUI, authMiddleware, "All requests", "JSON/HTTPS")

    Rel(authMiddleware, advisoryEndpoints, "Routes to")
    Rel(authMiddleware, sbomEndpoints, "Routes to")
    Rel(authMiddleware, purlEndpoints, "Routes to")
    Rel(authMiddleware, vulnEndpoints, "Routes to")
    Rel(authMiddleware, productEndpoints, "Routes to")
    Rel(authMiddleware, analysisEndpoints, "Routes to")

    Rel(advisoryEndpoints, advisoryService, "Uses")
    Rel(sbomEndpoints, sbomService, "Uses")
    Rel(purlEndpoints, purlService, "Uses")
    Rel(vulnEndpoints, vulnService, "Uses")
    Rel(productEndpoints, productService, "Uses")
    Rel(analysisEndpoints, analysis, "Uses")

    Rel(advisoryService, queryEngine, "Uses")
    Rel(sbomService, queryEngine, "Uses")
    Rel(purlService, queryEngine, "Uses")
    Rel(vulnService, queryEngine, "Uses")

    Rel(advisoryService, graphService, "Uses")
    Rel(sbomService, graphService, "Uses")
    Rel(purlService, graphService, "Uses")
    Rel(vulnService, graphService, "Uses")
    Rel(productService, graphService, "Uses")

    Rel(advisoryEndpoints, ingestor, "Upload")
    Rel(sbomEndpoints, ingestor, "Upload")

    Rel(graphService, db, "Reads/writes", "SQL")

    UpdateRelStyle(authMiddleware, advisoryEndpoints, $offsetX="-100")
    UpdateRelStyle(authMiddleware, sbomEndpoints, $offsetX="-80")
```

## Level 4: Component Diagram - Ingestor Service

Shows how document ingestion and parsing works.

```mermaid
C4Component
    title Component Diagram - Ingestor Service

    Container(apiServer, "API Server", "Rust", "Receives uploads")
    Container(importer, "Importer Service", "Rust", "Scheduled imports")
    Container(storage, "Object Storage", "S3", "Document storage")
    ContainerDb(db, "PostgreSQL", "Database", "Parsed entities")
    Container(analysis, "Analysis Service", "Rust", "Graph cache")

    Container_Boundary(ingestor, "Ingestor Service") {
        Component(formatDetector, "Format Detector", "Pattern matching", "Detects SBOM/advisory format")

        Component(spdxParser, "SPDX Parser", "spdx-rs", "Parses SPDX SBOMs")
        Component(cyclonedxParser, "CycloneDX Parser", "cyclonedx-bom", "Parses CycloneDX SBOMs")
        Component(csafParser, "CSAF Parser", "csaf crate", "Parses CSAF advisories")
        Component(osvParser, "OSV Parser", "JSON parser", "Parses OSV advisories")
        Component(cveParser, "CVE Parser", "JSON parser", "Parses CVE records")

        Component(sbomGraph, "SBOM Graph Builder", "SeaORM", "Creates SBOM entities and relationships")
        Component(advisoryGraph, "Advisory Graph Builder", "SeaORM", "Creates advisory and vulnerability entities")
        Component(purlResolver, "PURL Resolver", "PURL hierarchy", "Resolves BasePurl, VersionedPurl, QualifiedPurl")
        Component(cpeResolver, "CPE Resolver", "CPE parsing", "Resolves CPE identifiers")
        Component(statusResolver, "Status Resolver", "Version ranges", "Determines affected package versions")

        Component(graphLoader, "Graph Cache Loader", "Async tasks", "Loads SBOMs into analysis cache")
    }

    Rel(apiServer, formatDetector, "Sends document bytes")
    Rel(importer, formatDetector, "Sends document bytes")

    Rel(formatDetector, spdxParser, "SPDX documents")
    Rel(formatDetector, cyclonedxParser, "CycloneDX documents")
    Rel(formatDetector, csafParser, "CSAF documents")
    Rel(formatDetector, osvParser, "OSV documents")
    Rel(formatDetector, cveParser, "CVE documents")

    Rel(spdxParser, sbomGraph, "Parsed SBOM")
    Rel(cyclonedxParser, sbomGraph, "Parsed SBOM")
    Rel(csafParser, advisoryGraph, "Parsed advisory")
    Rel(osvParser, advisoryGraph, "Parsed advisory")
    Rel(cveParser, advisoryGraph, "Parsed advisory")

    Rel(sbomGraph, purlResolver, "Package identifiers")
    Rel(sbomGraph, cpeResolver, "CPE identifiers")
    Rel(advisoryGraph, purlResolver, "Affected packages")
    Rel(advisoryGraph, statusResolver, "Version ranges")

    Rel(sbomGraph, db, "Writes entities", "SQL")
    Rel(advisoryGraph, db, "Writes entities", "SQL")
    Rel(purlResolver, db, "Writes/reads", "SQL")
    Rel(cpeResolver, db, "Writes/reads", "SQL")
    Rel(statusResolver, db, "Writes", "SQL")

    Rel(formatDetector, storage, "Stores original", "S3 API")
    Rel(sbomGraph, graphLoader, "Triggers load")
    Rel(graphLoader, analysis, "Loads into cache")

    UpdateRelStyle(formatDetector, spdxParser, $offsetY="-20")
    UpdateRelStyle(formatDetector, cyclonedxParser, $offsetY="-10")
```
