# seiya-contracts

Primera pieza del núcleo Rust, no un reemplazo del servidor Python.

- Tipos JSON estrictos del contrato `1.0-draft.1`.
- Validación básica de versión, IDs, mensajes y tamaño serializado.
- Comprobación de selección de una herramienta anunciada para la plataforma,
  con efectos concedidos y confirmación cuando se exige.
- Ejemplos compartidos con `contracts/v1/` y pruebas de regresión.

```sh
cargo test --workspace --locked
cargo build --workspace --release --locked
```

No ejecuta modelos, archivos, red, Mail, shell, UI automation ni herramientas.
No implementa JSON Schema completo, validación de rutas/URLs, scopes, expiración
de grants, autenticación o sandbox. No es seguro añadir ejecución real detrás
de este helper sin implementar esas capas.

El catálogo incluido es **planned**, y se rechaza hasta que una implementación
del host lo marque realmente disponible tras validar su capacidad y permisos.
Los grants y la confirmación se proporcionan desde estado confiable del host,
nunca desde un objeto arbitrario del modelo o de una web externa.

Este crate no incluye un servidor. El servidor experimental de discovery está
en `crates/seiya-server`, sin inferencia todavía. No hay bindings Swift/FFI,
binarios iOS ni mediciones que acrediten una mejora frente al coordinador Python.
