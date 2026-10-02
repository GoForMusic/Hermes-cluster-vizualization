{{- define "hermes.name" -}}{{ .Release.Name }}-hermes{{- end -}}
{{- define "hermes.labels" -}}
app.kubernetes.io/name: hermes
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}
{{- define "hermes.selector" -}}
app.kubernetes.io/name: hermes
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
{{- define "hermes.secretName" -}}{{ default (include "hermes.name" .) .Values.secret.existingSecret }}{{- end -}}
{{- define "hermes.claimName" -}}{{ default (include "hermes.name" .) .Values.persistence.existingClaim }}{{- end -}}
