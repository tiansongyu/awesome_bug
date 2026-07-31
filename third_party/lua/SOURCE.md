# Lua source provenance

- Version: 5.4.8
- Upstream: <https://www.lua.org/ftp/lua-5.4.8.tar.gz>
- SHA-256:
  `4f18ddae154e793e46eeab727c59ef1c0c0c2b744e7b94219710d76f530629ae`

`CMakeLists.txt`, `LICENSE`, and this provenance note are project additions;
`src/lua.c` and `src/luac.c` are retained for source completeness but are not
linked into the embedded runtime.

The project carries one intentional ABI patch in `src/luaconf.h`: Lua integers
remain at the upstream 64-bit default while `lua_Number` uses `float`. The
desktop motion and SDL coordinate pipeline is single precision, so this avoids
silent double-to-float threshold drift in deterministic behavior replays. All
code linked to Lua includes this vendored header, making the ABI choice
consistent.
