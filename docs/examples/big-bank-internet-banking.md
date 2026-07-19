# Пример: Big Bank plc — Internet Banking System

Канонический C4 walkthrough Simon Brown / [c4model.com](https://c4model.com/),
который можно завести через MCP-инструменты architect-c4.

## Источники

- Официальный сайт C4: <https://c4model.com/>
- Интерактивный пример: <https://c4model.com/example>
- Разбор: [Applying the C4 Model to the Internet Banking System](https://www.cybermedian.com/a-comprehensive-step-by-step-case-study-applying-the-c4-model-to-the-internet-banking-system-big-bank-plc/)

## Viewer

- Context: <https://c4.example.com/?layer=context>
- Containers (IBS): <https://c4.example.com/?layer=container&parent=ibs>
- Components (API): <https://c4.example.com/?layer=component&parent=api>
- Code (Security): <https://c4.example.com/?layer=code&parent=security>
- ADRs: <https://c4.example.com/adrs>

### Агент

```text
get_view_links()
```

## Что смоделировано

1. **Context** — Customer, Internet Banking System, Mainframe (external), E-mail (external)
2. **Containers** — Web App, SPA, Mobile App, API Application, Database
3. **Components** (API) — Sign In / Accounts / Reset controllers, Security, Mainframe Facade, E-mail Component
4. **Code** (Security) — `PasswordEncoder`, `BcryptPasswordEncoder`, `AuthenticationService`
5. **ADR** — API is the only writer to the Database
