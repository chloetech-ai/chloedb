# Google Cloud answers, for the tests

What Google answered a read-only probe of a project on 2026-11-15, cut down
to what the tests read. The project's number is replaced by `123456789012`.

| file | call | what was kept |
|---|---|---|
| `machine-types.json` | `GET compute/v1/projects/P/zones/us-central1-a/machineTypes?maxResults=500` | the E2, N2, N2D, C3, C3D, C4 and N4 types of 2 to 16 vCPUs, as answered; then five made up to test the filters: `t2a-standard-4` (Arm), `g2-standard-4` (a GPU), `n1-standard-2` and `-4`, and a deprecated `n1-highmem-96` |
| `region.json` | `GET compute/v1/projects/P/regions/us-central1` | its zones, and the quotas a plan reads, with their limits as answered |
| `project.json` | `GET compute/v1/projects/P` | the project's quotas (`CPUS_ALL_REGIONS` 12) |
| `server-config.json` | `GET container/v1/projects/P/locations/us-central1/serverConfig` | as answered, less the image types' details; GKE's default (1.36.4) is a release channel's and not one of `validMasterVersions` |
| `skus.json` | `GET cloudbilling/v1/services/6F81-5844-456A/skus?currencyCode=USD` | 60 of the 208 SKUs of us-central1, and the two of `global` that price public addresses: the prices and descriptions as answered, the SKU ids made up, and one rate split into a free tier and a paid one |
