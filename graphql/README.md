# GraphQL API for trustify

`query OneSbom {getSbomById(id:"4ad38204-b998-4054-8ddc-a5c94ec37aa9") {sbomId, location, sha256, authors}}`

`query Sboms_by_location {sbomsByLocation(location: "1") {sbomId, location, sha256, authors}}`