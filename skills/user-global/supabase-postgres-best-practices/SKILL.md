---
name: supabase-postgres-best-practices
description: Postgres best practices for Supabase: indexes, RLS, migrations, performance.
---

# Supabase Postgres Best Practices
- Index foreign keys and filter columns used in RLS.
- Avoid unbounded `select *` in hot paths.
- Use transactions for multi-row writes.
