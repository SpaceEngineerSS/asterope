# asterope-ffi

The C interface to [Asterope](https://github.com/SpaceEngineerSS/asterope), a star tracker: a header
(`include/asterope.h`) and a static or shared library. A frame goes in, an
attitude quaternion comes out.

`cargo build --release -p asterope-ffi` builds the library;
`examples/solve.c` is a complete program. The guide is
[docs/integration.md](https://github.com/SpaceEngineerSS/asterope/blob/main/docs/integration.md).
