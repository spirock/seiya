# Dependencias y procedencia de Seiya

El árbol activo ya no incluye el motor, kernels, interfaces o paquetes del
proyecto anterior. Sus avisos originales se conservan en el historial Git y
en la copia local anterior a la limpieza; no se atribuye ese trabajo a Seiya.
El repositorio público Seiya parte de un snapshot nuevo: el historial y backup
del legado se conservan solo localmente, no se distribuyen con ese snapshot.
El texto Apache-2.0 de `LICENSE` se conserva.

## Núcleo Rust

- `serde` / `serde_derive`: MIT OR Apache-2.0.
- `serde_json`: MIT OR Apache-2.0.
- Las versiones y dependencias transitivas quedan fijadas en `Cargo.lock`.
  Revisar sus avisos antes de distribuir binarios.

## Servidor Rust experimental

Declaraciones de licencia verificadas en metadata de las versiones resueltas:

- Axum 0.8.9 y Tokio 1.53.3: MIT.
- libc 0.2.191: MIT OR Apache-2.0; bindings macOS usados directamente por
  seiya-observe para metadata y listado de hijos propios. No se copia código XNU.
- subtle 2.6.1: BSD-3-Clause.
- Tower 0.5.3 y http-body-util 0.1.5: MIT (dependencias de tests directas;
  algunas también forman parte del grafo transitivo HTTP).
- Reqwest 0.12.28: MIT OR Apache-2.0 (solo HTTP local/JSON; sin TLS).
- UUID 1.28.0: Apache-2.0 OR MIT (generación de IDs v4).
- tokio-rustls 0.26.6: MIT OR Apache-2.0; rustls 0.23.46: Apache-2.0 OR ISC OR MIT.
- webpki-roots 1.0.11: CDLA-Permissive-2.0; ring 0.17.14: Apache-2.0 AND ISC.
- html5ever / markup5ever: MIT OR Apache-2.0; tokenizer HTML usado sin copiar
  un motor externo. Declaraciones exactas de cada transitiva en su manifest.

No se copió implementación de servidores o herramientas del proyecto anterior.
La resolución completa está fijada en `Cargo.lock`; generar inventario de
dependencias/avisos transitivos y revisión de vulnerabilidades antes de una
distribución. Esta comprobación de metadata no sustituye esa auditoría.

## Componentes externos, no incluidos en los instaladores actuales

- Python, runtimes MLX/MLX-LM u Ollama y modelos descargados por separado.
- `curl` del sistema para la búsqueda web del prototipo.
- OpenCode es un cliente externo; no se ha copiado su implementación de tools.

Las licencias de pesos y runtimes son independientes. Antes de empaquetar un
modelo, runtime, framework de UI o código de terceros, verificar la licencia
exacta, redistribución, atribución, recursos y dependencias de la versión elegida.
Este archivo no certifica compatibilidad legal de componentes aún no elegidos.
