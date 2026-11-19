# lekalo-python-fastmcp

Framework-пакет FastMCP для [Lekalo](https://github.com/ichinya/lekalo), подключаемый внутри процесса языкового адаптера [lekalo-python](https://github.com/ichinya/lekalo-python) через versioned provider API.

- Извлечение observed evidence регистрации MCP tools и handlers — [#1](https://github.com/ichinya/lekalo-python-fastmcp/issues/1).
- Библиотечный пакет с manifest и схемами — собственный демон не требуется.
- Общий план отделения — [ichinya/lekalo#241](https://github.com/ichinya/lekalo/issues/241).

## Portable rules

Select `gh/ichinya/lekalo-python-fastmcp` in the component rules. The package supplies `contracts/tools.json` for `mcp.tools/v1`. This opt-in check requires at least one uniquely named tool registered by a direct @server.tool() decorator in main.py or server.py.

## Observed tool registrations

`evidence/` supplies the `fastmcp-tools` evidence package for Target
Protocol 1.4.0. The provider recognizes direct `@server.tool()` and
`@server.tool(name="literal")` registrations on a module-level server
constructed from a direct FastMCP import. Native server and handler
identities, the registration location and exact source digest accompany
each fact. Unsupported registration paths make coverage partial.
