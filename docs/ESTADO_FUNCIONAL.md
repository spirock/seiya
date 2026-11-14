# Estado funcional · fuentes locales

Esta tabla describe las fuentes actuales; para cada instalador verificar su SHA
en BUILD-INFO.json y el run del bloque Descargas de README.
Implementación, mocks y ensayos reales se distinguen. La tabla de paquetes en
README sigue describiendo el build distribuido anteriormente.

| Área | Implementado | Validación y pendiente |
|---|---|---|
| Identidad del SO | Driver Rust común, estado y detector Ollama | Fixtures de SO/arquitectura; no certifica otro target por compilar en macOS |
| Ollama | Rutas conocidas, override editable, aviso sin fallback | Archivos/fixtures; falta ensayo con instalaciones reales |
| Texto intercambiable | Catálogo, cerebro/trabajadores, bindings exactos, round-robin | HTTP/recursos simulados; no todos los modelos ni GPU reales |
| Imagen intercambiable | Catálogo image_models y selección CAS | Fixtures; necesita motor preparado con contrato de imagen. PNG nativo macOS |
| Imagen desde chat | Opt-in image_generate, registro idempotente y panel/progreso/PNG; también perfil nativo con chat_images | Cerebro/imagen sintéticos; falta eficacia con modelos reales. Perfil viejo continúa manual hasta revisión |
| Imágenes nativas | Pipeline Qwen macOS, control manual y chat opt-in | Pins conservados, ensayos anteriores no validan integración nueva |
| Búsqueda web | Opt-in, búsqueda/lectura/citas acotadas y cancelación | Fixtures; falta Internet y síntesis real en la integración nueva |
| Recursos y exclusión | Admisión, slot texto/imagen, locks y journals persistentes | Mocks/HTTP sintético; no mide VRAM ni confirma unload |
| Residencia nativa | Driver Ollama+generador propio secuencial, cierre de procesos y journal durable | Fixtures y helper propio; no VRAM medida ni sesión con modelos reales. Motores HTTP externos arbitrarios siguen sin lifecycle gestionado |
| Tools extensibles | Diseño de descubrimiento/revisión/permisos | Loader no implementado; copiar un archivo no ejecuta una tool |
| Visión/control PC | No implementado | Requiere adaptadores y permisos explícitos independientes |
| Linux/Windows nuevos | Código compartido y pruebas de contratos | Builds nativos, GUI e inferencia del cambio pendientes |
| Distribución | Instaladores generados por Actions, identificados por SHA/run | Descargar solo el build indicado; instalación/feedback reales y firma pendientes |

HTML/CSS/JS y diseño se conservan. El incremento de imágenes conversacionales
añade un puente JS acotado al panel existente, sin modificar los seis assets
congelados. Sin Docker ni gestor de ventanas nuevo. No hay garantía de memoria
suficiente ni residencia múltiple: los motores externos conservan esa autoridad.

Regresión offline actual:496 Rust,194 Python,5 suites Node correctas;24 ignored.
fmt, Clippy y diff-check correctos; seis hashes congelados intactos. No sustituye
pruebas con modelos, motores o dispositivos reales.
