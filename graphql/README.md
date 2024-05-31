# GraphQL API for trustify

## Run the graphql API server
 `cargo run --bin trustify-graphql`

## Advisory Entity Queries
Fetch all advisories and each related organization and vulnerabilities :
`curl -s localhost:9090 -H "Content-Type: application/json" -d '{ "query": "{ getAdvisories { id identifier location sha256 published organization { id website } vulnerabilities { id identifier title }}}" }' `

Fetch an advisory and its related organization and vulnerabilities :
`curl -s localhost:9090 -H "Content-Type: application/json" -d '{ "query": "{ getAdvisoryById(id: 1) { id identifier location sha256 published organization { id website } vulnerabilities { id identifier title }}}" }'` 

## Organization Entity Queries
Fetch an organization by its name :
`curl -s localhost:9090 -H "Content-Type: application/json" -d '{ "query": "{ getOrganizationByName(name: \"org1\" ) { id name cpeKey website}}" }' `

## SBOM Entity Queries
Fetch all SBOMs by location :
`query Sboms_by_location {sbomsByLocation(location: "1") {sbomId, location, sha256, authors}}`

Fetch a SBOM by its Id :
`query OneSbom {getSbomById(id:"4ad38204-b998-4054-8ddc-a5c94ec37aa9") {sbomId, location, sha256, authors}}`