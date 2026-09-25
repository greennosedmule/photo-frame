{{- define "photoframe.name" -}}
{{- default .Release.Name .Values.nameOverride | trunc 50 | trimSuffix "-" -}}
{{- end -}}

{{- define "photoframe.labels" -}}
app.kubernetes.io/name: photoframe
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- /* Flux appends "+<digest>" to the version of a chart from an OCI source, and "+" is invalid in a label value. */}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end -}}

{{/* selector labels for a component: web or indexer. The sqlite pod carries both. */}}
{{- define "photoframe.selector" -}}
app.kubernetes.io/name: photoframe
app.kubernetes.io/instance: {{ .root.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{- define "photoframe.isSqlite" -}}
{{- if eq .Values.database.type "sqlite" -}}true{{- end -}}
{{- end -}}

{{- define "photoframe.validate" -}}
{{- if not (has .Values.database.type (list "sqlite" "postgres")) -}}
{{- fail "database.type must be sqlite or postgres" -}}
{{- end -}}
{{- if not .Values.image.registry -}}
{{- fail "image.registry is required, e.g. --set image.registry=ghcr.io/<owner>" -}}
{{- end -}}
{{- if and (eq .Values.database.type "sqlite") (gt (int .Values.web.replicas) 1) -}}
{{- fail "web.replicas must be 1 with database.type=sqlite; use postgres to scale web" -}}
{{- end -}}
{{- if and (eq .Values.database.type "postgres") (not .Values.database.postgres.url) (not .Values.database.postgres.existingSecret) -}}
{{- fail "database.postgres.url or database.postgres.existingSecret is required with database.type=postgres" -}}
{{- end -}}
{{- if and (not .Values.admin.password) (not .Values.admin.existingSecret) (not .Values.admin.allowedCIDRs) -}}
{{- fail "admin.password, admin.existingSecret or admin.allowedCIDRs is required" -}}
{{- end -}}
{{- end -}}

{{- define "photoframe.secretName" -}}{{ include "photoframe.name" . }}{{- end -}}
{{- define "photoframe.adminSecretName" -}}{{ default (include "photoframe.secretName" .) .Values.admin.existingSecret }}{{- end -}}
{{- define "photoframe.adminSecretKey" -}}{{ if .Values.admin.existingSecret }}{{ .Values.admin.existingSecretKey }}{{ else }}admin-password{{ end }}{{- end -}}
{{- define "photoframe.dbSecretName" -}}{{ default (include "photoframe.secretName" .) .Values.database.postgres.existingSecret }}{{- end -}}
{{- define "photoframe.dbSecretKey" -}}{{ if .Values.database.postgres.existingSecret }}{{ .Values.database.postgres.existingSecretKey }}{{ else }}database-url{{ end }}{{- end -}}
{{- define "photoframe.libraryClaim" -}}{{ default (printf "%s-library" (include "photoframe.name" .)) .Values.library.persistence.existingClaim }}{{- end -}}
{{- define "photoframe.stateClaim" -}}{{ default (printf "%s-state" (include "photoframe.name" .)) .Values.database.sqlite.persistence.existingClaim }}{{- end -}}

{{- define "photoframe.publicHostname" -}}
{{- if .Values.publicHostname -}}{{ .Values.publicHostname }}
{{- else if .Values.ingress.hosts -}}{{ first .Values.ingress.hosts }}
{{- end -}}
{{- end -}}

{{- define "photoframe.image" -}}
{{ printf "%s/photoframe-%s:%s" (trimSuffix "/" .root.Values.image.registry) .component .root.Values.image.tag }}
{{- end -}}

{{- define "photoframe.dbEnv" -}}
- name: DATABASE_URL
{{- if eq .Values.database.type "sqlite" }}
  value: sqlite:///state/photoframe.db
{{- else }}
  valueFrom:
    secretKeyRef:
      name: {{ include "photoframe.dbSecretName" . }}
      key: {{ include "photoframe.dbSecretKey" . }}
{{- end }}
- name: LIBRARY_ROOT
  value: {{ .Values.library.mountPath | quote }}
- name: LOG_LEVEL
  value: {{ .Values.logLevel | quote }}
{{- end -}}

{{- define "photoframe.webContainer" -}}
- name: web
  image: {{ include "photoframe.image" (dict "root" . "component" "web") | quote }}
  imagePullPolicy: {{ .Values.image.pullPolicy }}
  ports:
    - {name: http, containerPort: 8080}
  env:
    {{- include "photoframe.dbEnv" . | nindent 4 }}
    - name: ADMIN_USERNAME
      value: {{ .Values.admin.username | quote }}
    {{- if or .Values.admin.password .Values.admin.existingSecret }}
    - name: ADMIN_PASSWORD
      valueFrom:
        secretKeyRef:
          name: {{ include "photoframe.adminSecretName" . }}
          key: {{ include "photoframe.adminSecretKey" . }}
    {{- end }}
    {{- with .Values.admin.allowedCIDRs }}
    - name: ADMIN_ALLOWED_CIDRS
      value: {{ join "," . | quote }}
    {{- end }}
    {{- with .Values.admin.trustedProxyCIDRs }}
    - name: TRUSTED_PROXY_CIDRS
      value: {{ join "," . | quote }}
    {{- end }}
    - name: MAX_UPLOAD_BYTES
      value: {{ .Values.web.maxUploadBytes | int64 | quote }}
    {{- with include "photoframe.publicHostname" . }}
    - name: PUBLIC_HOSTNAME
      value: {{ . | quote }}
    {{- end }}
    {{- with .Values.web.extraEnv }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  readinessProbe:
    httpGet: {path: /healthz, port: http}
    periodSeconds: 10
  livenessProbe:
    httpGet: {path: /healthz, port: http}
    initialDelaySeconds: 10
    periodSeconds: 20
  resources: {{- toYaml .Values.web.resources | nindent 4 }}
  securityContext: {{- toYaml .Values.containerSecurityContext | nindent 4 }}
  volumeMounts:
    - {name: library, mountPath: {{ .Values.library.mountPath | quote }}}
    {{- if eq .Values.database.type "sqlite" }}
    - {name: state, mountPath: /state}
    {{- end }}
{{- end -}}

{{- define "photoframe.indexerContainer" -}}
- name: indexer
  image: {{ include "photoframe.image" (dict "root" . "component" "indexer") | quote }}
  imagePullPolicy: {{ .Values.image.pullPolicy }}
  env:
    {{- include "photoframe.dbEnv" . | nindent 4 }}
    - {name: SCAN_INTERVAL_SECS, value: {{ .Values.indexer.scanIntervalSecs | int64 | quote }}}
    - {name: INGEST_REQUIRE_SCAN, value: {{ .Values.indexer.ingestRequireScan | quote }}}
    - {name: INGEST_QUIET_SECS, value: {{ .Values.indexer.ingestQuietSecs | int64 | quote }}}
    - {name: DERIVATIVE_WORKERS, value: {{ .Values.indexer.derivativeWorkers | int64 | quote }}}
    - {name: MAX_UPLOAD_BYTES, value: {{ .Values.web.maxUploadBytes | int64 | quote }}}
    - {name: MAX_DECODE_PIXELS, value: {{ .Values.indexer.maxDecodePixels | int64 | quote }}}
    - {name: MAX_DECODE_BYTES, value: {{ .Values.indexer.maxDecodeBytes | int64 | quote }}}
    - {name: DECODE_TIMEOUT_SECS, value: {{ .Values.indexer.decodeTimeoutSecs | int64 | quote }}}
    - {name: ENABLE_AVIF, value: {{ .Values.indexer.enableAvif | quote }}}
    {{- with .Values.indexer.extraEnv }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  resources: {{- toYaml .Values.indexer.resources | nindent 4 }}
  securityContext: {{- toYaml .Values.containerSecurityContext | nindent 4 }}
  volumeMounts:
    - {name: library, mountPath: {{ .Values.library.mountPath | quote }}}
    {{- if eq .Values.database.type "sqlite" }}
    - {name: state, mountPath: /state}
    {{- end }}
{{- end -}}

{{- define "photoframe.volumes" -}}
- name: library
  persistentVolumeClaim:
    claimName: {{ include "photoframe.libraryClaim" . }}
{{- if eq .Values.database.type "sqlite" }}
- name: state
  persistentVolumeClaim:
    claimName: {{ include "photoframe.stateClaim" . }}
{{- end }}
{{- end -}}
