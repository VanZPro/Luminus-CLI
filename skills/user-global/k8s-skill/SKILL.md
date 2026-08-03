---
name: k8s-skill
description: Kubernetes manifests, Deployments, Services, Ingress, Kustomize, debugging pods.
---

# Kubernetes Skill
- Start from Deployment + Service; add HPA/Ingress only when needed.
- Always set resources requests/limits and probes.
- Prefer Kustomize overlays over duplicated YAML.
- Debug: `kubectl get/describe/logs`, events, `kubectl auth can-i`.
- Do not apply from memory — read live manifests first when possible.
