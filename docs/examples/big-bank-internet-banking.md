# Example: Big Bank plc — Internet Banking System

Canonical C4 walkthrough (Simon Brown / [c4model.com](https://c4model.com/)) built
live in **architect-c4**.

## Sources

- Official C4 site: <https://c4model.com/>
- Interactive example: <https://c4model.com/example>
- Case study article: [Applying the C4 Model to the Internet Banking System](https://www.cybermedian.com/a-comprehensive-step-by-step-case-study-applying-the-c4-model-to-the-internet-banking-system-big-bank-plc/)
- Skills in this repo:
  - `.cursor/skills/c4-architect-modeling/`
  - `.cursor/skills/architecture-decision-records/`

## Live workspace

| Field | Value |
|-------|-------|
| workspace_id | `8c3b4690-cd3d-449f-8de0-484ae88829d0` |
| project | `big-bank-plc` |

### Viewer links

- Context: <https://c4.example.com/view/8c3b4690-cd3d-449f-8de0-484ae88829d0?layer=context>
- Containers (IBS): <https://c4.example.com/view/8c3b4690-cd3d-449f-8de0-484ae88829d0?layer=container&parent=ibs>
- Components (API): <https://c4.example.com/view/8c3b4690-cd3d-449f-8de0-484ae88829d0?layer=component&parent=api>
- Code (Security): <https://c4.example.com/view/8c3b4690-cd3d-449f-8de0-484ae88829d0?layer=code&parent=security>
- ADRs: <https://c4.example.com/view/8c3b4690-cd3d-449f-8de0-484ae88829d0/adrs>

### Agent

```text
get_view_links("8c3b4690-cd3d-449f-8de0-484ae88829d0")
```

## What was modeled

1. **Context** — Customer, Internet Banking System, Mainframe (external), E-mail (external)
2. **Containers** — Web App, SPA, Mobile App, API Application, Database
3. **Components** (API) — Sign In / Accounts / Reset controllers, Security, Mainframe Facade, E-mail Component
4. **Code** (Security) — `PasswordEncoder`, `BcryptPasswordEncoder`, `AuthenticationService`
5. **ADR** — API is the only writer to the Database
