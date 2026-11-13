# ADR 0001 · servidor Rust experimental

- Fecha: 2026-10-19.
- Estado: aceptada para el esqueleto de S1-02, no como migración de producción.
- Alcance: CLI desktop y discovery HTTP; inferencia y broker no incluidos.

Evolución: ADR 0002 añade el adaptador interno y ADR 0003 el chat JSON opt-in.
Este documento conserva las decisiones y límites originales de S1-02.

## Contexto

Se necesita un binario opt-in que pueda evolucionar hacia el camino fast de
OpenCode, con tests HTTP y configuración sin secretos embebidos. Python sigue
en uso. No se debe reemplazar su servidor, su puerto 8010 ni el runtime MLX.

## Elección

- **Axum 0.8**, sin features por defecto, con HTTP/1, JSON y Tokio: routing,
  tipos HTTP y middleware. Evitar un parser HTTP casero o un framework con
  componentes de UI/WebSocket que todavía no necesitamos.
- **Tokio 1**, features explícitas para runtime, red, macros y señales: servidor
  concurrente y cierre ordenado. La concurrencia HTTP no autoriza inferencias
  paralelas; el scheduler se implementará en S1-04.
- **Serde / serde_json**, ya presentes, para configuración y respuestas.
- **subtle 2** para comparar credenciales con una primitiva de tiempo constante,
  sin escribir una comparación criptográfica propia.
- Tests de router con **Tower util** y **http-body-util**; lifecycle con listener
  loopback efímero y Tokio. Son dependencias de desarrollo.
- CLI pequeña con argumentos estándar: `serve --config PATH`, `check-config`,
  `--help`, `--version`. No añadir Clap hasta que la complejidad lo justifique.
- **Cliente saliente diferido a S1-03**: evaluar reqwest con features mínimas,
  sin redirects ni proxies implícitos y endpoints locales validados. No añadir
  una dependencia de red todavía no utilizada ni conectar un runtime en S1-02.

Versiones transitivas exactas en `Cargo.lock`. Axum/Tokio/Tower/http-body-util
usan MIT; subtle BSD-3-Clause; Serde/serde_json MIT OR Apache-2.0. Verificar el
metadata real resuelto y avisos antes de distribuir; no equivale a auditoría
de vulnerabilidades ni aprobación para App Store.

## Interfaz y seguridad inicial

- Binario `seiya-server`, no sustituye el comando Python `seiya`.
- Configuración nueva y estricta, sin migrar ni leer automáticamente el config
  actual. `listen` debe ser `127.0.0.1:<puerto>`; ejemplo 8012. Puerto 0 solo
  sirve para pruebas/selección efímera y se informa la dirección real.
- El archivo puede nombrar una variable de entorno para la API key, pero no
  contener su valor. Si se declara esa variable y falta, está vacía o no es una
  credencial válida para HTTP, fallar cerrado. Sin esa opción, servicio loopback
  sin autenticación, declarado como tal.
- Validar Host/Origin incluso en discovery. No CORS abierto. Comparación Bearer
  sin logs de headers. Respuestas JSON con `Cache-Control: no-store`.
- `/health` y `/v1/models` usan las formas de las fixtures Python. Health no
  afirma disponibilidad del runtime ni haber cargado pesos.
- `/v1/chat/completions` devuelve 501 explícito. UI, `/api/chat` y protocolo
  `/native/v1` no existen. No ofertar herramientas nativas futuras.
- No se validan todavía payloads de chat ni se implementan cuotas/cola de
  inferencia: no se acepta ni ejecuta inferencia en esta fase.
- Arranque imprime dirección y estado experimental, no contenido del archivo,
  secretos o errores detallados de parsing. No request tracing por defecto.
- Cierre ordenado con Ctrl+C y, en Unix, SIGTERM. Si el puerto está ocupado,
  fallar; no matar al propietario, buscar otro puerto o instalar un servicio.

## Alternativas y consecuencias

- `std::net` + HTTP manual: menos dependencias aparentes, más superficie propia
  de parsing, lifecycle, seguridad y concurrencia; descartado.
- Usar solo un wrapper alrededor de Python: no valida servidor compilado ni
  avance de migración; descartado para este hito.
- Runtime/UI móvil ahora: agranda el alcance sin validar interoperabilidad;
  diferido. El crate servidor no declara soporte iOS por compilar en macOS.
- Coste: más dependencias y lockfile mayor; se justifican por infraestructura
  HTTP estándar. No se promete menor memoria ni más tokens/s por esta elección.

No modificar el servicio activo o proveedor de OpenCode para probar discovery.
La prueba contra HTTP local no acredita aún un ciclo de herramientas OpenCode.
