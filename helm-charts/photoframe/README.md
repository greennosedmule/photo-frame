# photoframe chart

Deploys `photoframe-web` and `photoframe-indexer`. `database.type` picks the topology (see docs/SPEC.md, *Deployment topology*).

| | `sqlite` (default) | `postgres` |
| --- | --- | --- |
| Workloads | 1 Deployment, 2 containers | 2 Deployments |
| Web replicas | 1 (enforced) | 1..n |
| Indexer | in the same pod | exactly 1, `Recreate` |
| Volumes | library + state (RWO is fine) | library, RWX |

## Install

The `image` workflow pushes the chart to GHCR alongside the images, so a cluster already configured to pull from `ghcr.io` can install directly by OCI reference — no `helm repo add` needed:

```bash
# create the pull secret first (see docs/SPEC.md, Pulling from GHCR)
helm install frame oci://ghcr.io/<owner>/charts/photoframe --version 0.1.0 -n photoframe --create-namespace \
  --set image.registry=ghcr.io/<owner> \
  --set 'imagePullSecrets[0].name=ghcr' \
  --set admin.password=<password> \
  --set ingress.enabled=true --set 'ingress.hosts={frame.example.com}'
```

Or from a checkout, with the chart's local path in place of the OCI reference:

```bash
helm install frame ./helm-charts/photoframe -n photoframe --create-namespace \
  --set image.registry=ghcr.io/<owner> \
  --set 'imagePullSecrets[0].name=ghcr' \
  --set admin.password=<password> \
  --set ingress.enabled=true --set 'ingress.hosts={frame.example.com}'
```

Postgres: add `--set database.type=postgres --set database.postgres.url=postgres://user:pass@host/db --set library.persistence.existingClaim=<rwx-claim>`.
Prefer `admin.existingSecret` and `database.postgres.existingSecret` over passing credentials on the command line.

## Notes

- The library and state PVCs carry `helm.sh/resource-policy: keep`; `helm uninstall` leaves your photos and database.
- Never put the SQLite state volume on NFS. If you need web on more than one node, switch to Postgres.
- Containers run as uid 65532 with a read-only root filesystem. On NFS, make the export writable for that uid, because `fsGroup` is not applied.
- `indexer.resources` assumes 2 workers. A 48 MP original peaks near 450 MB per worker.
- Not yet run against a real cluster or Postgres; it has only been linted and rendered with `helm template`.
