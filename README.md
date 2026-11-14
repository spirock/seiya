![Seiya · Pegassus — asistente local experimental](docs/assets/seiya-banner.svg)

# Seiya · Pegassus

Asistente local con una experiencia común y motores adaptados al dispositivo.
Repositorio público: [spirock/seiya](https://github.com/spirock/seiya).
**Seiya** es la aplicación/proveedor; **Pegassus**, el modelo público
(`seiya/pegassus` en OpenCode).

## Descargas

<!-- seiya-downloads:start -->
**Último build verificado:** [`e7d6cbc`](https://github.com/spirock/seiya/actions/runs/43585855303).

| Sistema | Descarga |
|---|---|
| Windows x64 | [Instalador .exe](https://github.com/spirock/seiya/actions/runs/43585855303/artifacts/14110776488) · [ZIP portable](https://github.com/spirock/seiya/actions/runs/43585855303/artifacts/14110726653) |
| macOS Apple Silicon | [.dmg / .pkg](https://github.com/spirock/seiya/actions/runs/43585855303/artifacts/14110087539) |
| Linux x64 | [.deb](https://github.com/spirock/seiya/actions/runs/43585855303/artifacts/14110591605) |
<!-- seiya-downloads:end -->

Actions actualiza este bloque al completar correctamente un nuevo build de `main`.
GitHub entrega los artefactos envueltos en ZIP, puede requerir iniciar sesión y
los enlaces caducan según su retención. No son releases firmadas ni incluyen modelos.
Windows/Linux comparten shell web y catálogo HTTP experimental; no incluyen la
inferencia nativa macOS ni motores. [Prueba del instalador y feedback](docs/PRUEBA_INSTALADOR.md).
[Requisitos y límites](docs/PAQUETES_PRUEBA.md) · [Windows](docs/WINDOWS_RUST.md).

![Capas de Seiya: experiencia web, coordinación Rust y adaptadores locales](docs/assets/seiya-arquitectura.svg)

## Funciones por sistema operativo

Esta tabla describe las **fuentes Rust actuales**, no el prototipo Python ni una
promesa de paridad. El bloque Descargas identifica el SHA/run verificado del
paquete ofrecido; un build anterior puede no contener las funciones nuevas.
**✅ disponible/comprobado en el alcance indicado · 🧪 implementado experimental,
con validación limitada · ❌ no disponible en ese paquete.**

| Característica | macOS Apple Silicon | Linux x64 | Windows x64 |
|---|:---:|:---:|:---:|
| Paquete generado en Actions | ✅ `.dmg` / `.pkg` | ✅ `.deb` | ✅ instalador `.exe` / ZIP |
| CLI: ayuda, versión y validación de configuración | ✅ | ✅ | ✅ |
| API local experimental | 🧪 | 🧪 | 🧪 |
| Integración con OpenCode mediante API compatible parcial | 🧪 ensayos aislados anteriores | 🧪 contratos/mocks | 🧪 contratos/mocks |
| Launcher con diálogos para configurar/abrir perfil | 🧪 | ❌ | ❌ |
| Shell web de configuración sin modelos y detección Ollama | 🧪 | 🧪 fixtures | 🧪 fixtures |
| Catálogo de cerebro/trabajadores y motor HTTP externo | 🧪 | 🧪 contratos/mocks | 🧪 contratos/mocks |
| Búsqueda web integrada opt-in | 🧪 fixtures | 🧪 fixtures | 🧪 fixtures |
| Chat local con modelo del ensayo | 🧪 Gemma | ❌ | ❌ |
| Configuración visual de imágenes/multiagente | 🧪 | ❌ | ❌ |
| Generación local de imágenes | 🧪 pipeline Qwen-Image | ❌ | ❌ |
| Trabajador auxiliar de borradores por turnos | 🧪 Qwen | ❌ | ❌ |
| Historial del chat en el navegador | 🧪 mismo origen | 🧪 contrato común | 🧪 contrato común |
| Recuperación de trabajos/PNG sin regenerar | 🧪 | ❌ | ❌ |
| Gestor de asignaciones del catálogo de texto | 🧪 | 🧪 | 🧪 |
| Imágenes desde chat y lifecycle nativo secuencial | 🧪 fixtures | ❌ | ❌ |
| Modelos/pesos incluidos o descargados automáticamente | ❌ | ❌ | ❌ |
| Instalación y GUI validadas en el equipo del usuario | ❌ | ❌ | ❌ |
| Firma/notarización o Authenticode de los paquetes | ❌ | ❌ | ❌ |

**Límites importantes:** macOS requiere motores/assets compatibles ya preparados;
el nuevo launcher tiene pruebas con diálogos/browser/host simulados, no GUI o
inferencia real acreditadas desde esa entrada. La API de Windows/Linux no equivale
a un backend de modelos nativo validado. Linux exige la glibc indicada en el `.deb`;
no se ofrecen aún builds verificados macOS Intel o Linux/Windows ARM64.
Acceder desde un navegador de otro SO al host LAN macOS es distinto de ejecutar
la interfaz/modelos localmente en ese SO. [Evidencia y diferencias](docs/PARIDAD_RUST.md).

**OpenCode:** integración mediante el ejecutable API `seiya-server` (en los
paquetes, `seiya-rust-server` o `seiya-server.exe`), con discovery y chat/tools
compatibles parcialmente con OpenAI. Dos ensayos reales aislados anteriores en
macOS no acreditan el nuevo launcher, sesiones sostenidas o Windows/Linux reales.
No es un plugin incluido ni una configuración automática de OpenCode; la web del
launcher no equivale por sí sola a ese servidor API. Las herramientas las ejecuta
OpenCode, no Seiya una segunda vez. [Evidencia y alcance](docs/PRUEBA_RUST_OPENCODE.md).

## Modelos y asignaciones

**Diseño objetivo completo (implementación parcial):** un cerebro central,
un pipeline de imágenes y un grupo configurable de trabajadores fuertes. Qwen,
DeepSeek u otros modelos compatibles podrán sustituirse por rol; cantidad y
residencia dependerán de los recursos medidos del equipo. Varios habilitados no
significa todos cargados simultáneamente. La tabla siguiente describe el perfil
actual del ensayo, no una lista de marcas obligatorias para el producto final.

**Pegassus** (`seiya/pegassus`) es la identidad pública de Seiya, no otro paquete
de pesos. El perfil experimental macOS trabaja con estos componentes:

| Modelo / variante | Asignación |
|---|---|
| **Gemma 4 E2B** · `gemma4:e2b-mlx` | Conversación principal y coordinación. Cuando multiagente está habilitado, puede solicitar y consultar borradores del auxiliar mediante tools estructuradas. |
| **Qwen 3.5 9B** · `qwen3.5:9b-mlx` | Auxiliar opcional de código: prepara borradores por turnos. No ejecuta código, escribe proyectos, pasa tests o navega por tener el switch activado. |
| **Qwen-Image-2.1-Turbo** · Unsloth `Q4_K_M` | Generación local de imágenes mediante el control explícito **Imagen**. No se activa por prosa ni se anuncia como tool conversacional ficticia. |
| **Qwen3-VL-8B-Instruct** · `UD-Q4_K_XL` | Codificador de texto del pipeline de imágenes; no es un agente adicional ni acredita visión conversacional. |
| **VAE Qwen-Image-2.1** · `BF16` | Componente de decodificación del pipeline de imágenes, no un modelo de chat. |

Gemma/Qwen usan el runtime compatible del ensayo macOS; imágenes usan una build
verificada de `stable-diffusion.cpp`. LLM e imagen alternan turnos bajo lease:
no se presupone que todos los modelos quepan residentes a la vez. Abrir/configurar
no genera ni descarga pesos. Los instaladores **no incluyen esos modelos**.

**Validación:** respuestas breves individuales Gemma/Qwen y una imagen512×512
probadas anteriormente en macOS, no una sesión conjunta sostenida ni el nuevo
launcher GUI. Windows/Linux no tienen inferencia nativa equivalente validada.
Pesos/revisiones/licencias y límites: [imágenes](docs/MODELO_IMAGENES_QWEN_2_1.md),
[operación](docs/APP_RUST.md) y [matriz de paridad](docs/PARIDAD_RUST.md).

## Estado real

**Implementación del snapshot actual, con validación real limitada:**
arranque web sin perfil/motores y catálogo de texto con cerebro/trabajadores
sustituibles. Conector HTTP local Rust compartido entre Windows/Linux/macOS,
para motores externos ya preparados; pool secuencial, bindings, recursos,
locks/journal y asignaciones CAS. Sin catálogo no se inventa un modelo disponible;
incluye búsqueda/lectura Rust opt-in del catálogo (consultas a DuckDuckGo) y
detección de archivos Ollama por SO con ruta editable. Probadas con fixtures,
no búsqueda/síntesis real ni compatibilidad de instalaciones Ollama.
El incremento siguiente añade catálogo de imágenes sustituibles y generación
desde el chat, mediante API local preparada y panel de trabajos/PNG existente.
[Estado general](docs/ESTADO_FUNCIONAL.md) · [operación de imágenes](docs/IMAGENES_DESDE_CHAT.md).
La ruta nativa macOS incorpora además lifecycle secuencial Ollama/imagen, journal
de intención y chat de imágenes opt-in con pins conservados; validación sintética,
no liberación física de VRAM acreditada. [Operación nativa](docs/RESIDENCIA_NATIVA.md).
macOS mantiene también su ruta MLX anterior. No hay motor/pesos incluidos ni
visión/control del PC, y faltan builds/GUI/inferencia reales del cambio por SO.
[Operación y límites](docs/BACKENDS_LOCALES.md). Las tablas de paquetes anteriores describen
el build verificado del bloque Descargas; revisar su SHA frente a estas fuentes.

- Prototipo funcional en [`swarm-app/`](swarm-app/README.md): Python, chat web,
  historial local, búsqueda web y API compatible parcialmente con OpenAI.
- Integración probada con OpenCode; worker único para herramientas, consenso
  explícito para consultas sin herramientas. No hay aún cooperación completa
  de agentes que ejecuten y revisen cambios.
- Instaladores de prueba: [Rust experimental y rollback Python](docs/PAQUETES_PRUEBA.md).
  Rust usa app/comandos separados, sin modelos/runtime incluidos ni paridad
  completa acreditada. El servidor activo no se sustituye automáticamente.
  Windows x64: [.exe/ZIP e instalador por usuario](docs/WINDOWS_RUST.md), CLI/API
  experimental, sin inferencia Windows o web nativa validadas.
- Nuevo núcleo **Rust**: tipos y validación básica del contrato en
  [`crates/seiya-contracts/`](crates/seiya-contracts/README.md). **No sustituye
  todavía al coordinador Python ni implementa herramientas del sistema.**
- Servidor Rust experimental [`seiya-server`](crates/seiya-server/README.md):
  discovery por defecto y **chat JSON/SSE bufferizado opt-in**, con cola y límites.
  Probado con mocks y dos smoke tests reales aislados OpenCode/Gemma: lectura,
  edición y tests. Sin tokens en vivo; no sustituye el servicio Python.
- Visión, Mail, control de aplicaciones, app iOS e interfaz multiplataforma
  empaquetada: **especificados, no implementados**. Phi es un candidato móvil,
  no una elección validada para el iPhone 15 Pro Max.
- Trabajos de borradores en segundo plano: broker Rust opt-in, estado/cancelación
  y panel web, con vista previa **simulada** y pruebas de conversación mientras
  trabaja el auxiliar falso. No ejecuta código, escribe proyectos o acredita
  concurrencia Gemma/Qwen real. [Operación y límites](docs/TRABAJOS_EN_SEGUNDO_PLANO.md).
- Imágenes locales en evaluación: una imagen512×512 real validada con supervisor,
  downloader y validación PNG en Rust/macOS. Broker/API/control web opt-in de
  imágenes implementados con pruebas sintéticas, **no activados ni probados con
  nueva inferencia**; sin presupuesto Gemma/Qwen/imagen conjunto validado.
  [Operación y límites](docs/IMAGEN_LOCAL_EXPERIMENTAL.md).

Host temporal chat+imágenes/LAN autorizado por separado: `seiya-image-web`,
residencia alternada, TLS1.3 y autenticación de un uso, sin cambiar el servidor
principal. Estado de arranque y validaciones (no garantía de servicio permanente):
[operación](docs/IMAGEN_LOCAL_EXPERIMENTAL.md).

[Historial de trabajos/imágenes](docs/HISTORIAL_TRABAJOS.md): snapshots Rust
opt-in, recuperación al recargar/reiniciar sin volver a generar y mismo diseño.
Conversaciones siguen en el navegador; no sincronización entre PCs.

[LAN y diagnóstico sin inferencia](docs/DIAGNOSTICO_LAN.md): host temporal TLS
separado y panel de límites/observaciones Rust. Guardar límites no activa modelos
ni levanta el bloqueo por presión crítica del ensayo real.

## Contrato de producto y arquitectura

1. [Contrato técnico completo](docs/CONTRATO_SEIYA.md)
2. [Protocolo y estados](docs/PROTOCOLO_V1.md)
3. [Entrega, pruebas y migración a Rust](docs/ENTREGA_Y_MIGRACION.md)
4. [Contratos legibles por máquina](contracts/v1/)
5. [Evidencia y límites del prototipo](swarm-app/VERIFICATION.md)
6. [Memoria adaptativa para Mac y reutilización selectiva](docs/MEMORIA_ADAPTATIVA.md)
7. [Fixtures de compatibilidad OpenCode/OpenAI](contracts/openai-compat/v0.1.0/README.md)
8. [Prueba real aislada Rust/OpenCode y límites](docs/PRUEBA_RUST_OPENCODE.md)
9. [Operación experimental, diferencias y rollback](docs/OPERACION_RUST_EXPERIMENTAL.md)
10. [Diseño de baseline de memoria, aún no ejecutada](docs/BASELINE_MEMORIA.md)
11. [Contrato de chat estable y lectura web, pendiente de implementación](docs/CONTRATO_CHAT_ESTABLE_Y_WEB.md)

## Desarrollo

**Frontend permanente:** HTML/CSS/JavaScript y diseño existentes se conservan;
no se portarán a Rust ni se sustituirán por otro framework. Assets congelados
durante los incrementos de backend, con regresión SHA256. La lógica nueva
compartida y los adaptadores de modelos pertenecen al backend Rust.

Migración actual: [aplicación Rust instalable](docs/APP_RUST.md). Primer incremento:
`seiya-desktop --check-config` valida configuración sin iniciar runtimes; entrada
manual macOS `--serve` con rutas configuradas y coordinación compartida, probada
offline/helpers. Todavía no sustituye el launcher Python ni acredita inferencia
real desde esa entrada nueva. Diseño web conservado.

[Guía de contribución](CONTRIBUTING.md) · [Hoja de ruta pública](ROADMAP.md).
Las pruebas normales usan fixtures y adaptadores simulados. Ensayos reales de
modelos, instalación, permisos del SO o servicios se validan por separado; no
se consideran completados por compilar los paquetes.

```sh
make test-python
make test-rust
make serve
```

`make serve` necesita una configuración Seiya válida y el runtime local ya
arrancado; no descarga pesos ni configura permisos del sistema.

## Estructura

```text
crates/seiya-contracts/   Tipos y políticas básicas del contrato Rust
crates/seiya-server/      Servidor experimental: discovery o chat JSON opt-in
crates/seiya-observe/     Observaciones/probes, runner y driver opt-in; no baseline MLX medida
contracts/v1/            Esquema, catálogo y ejemplos del contrato propuesto
docs/                    Especificación y criterios de aceptación
ROADMAP.md               Estado del producto y próximos hitos públicos
swarm-app/               Aplicación ejecutable actual y empaquetado
local-tools-tests/       Benchmarks y evidencia de selección de modelos
```

El motor y las interfaces anteriores se retiraron del árbol de trabajo.
El historial Git anterior se conserva solo localmente; la publicación Seiya
parte de un snapshot sin el historial ajeno. La limpieza no borra modelos, instalaciones,
conversaciones ni configuración externa. Detalles en
[la nota de limpieza](docs/LIMPIEZA.md).

## Compilación y privacidad

El objetivo es un núcleo compilado en Rust y adaptadores nativos por plataforma.
Un ejecutable **no es secreto ni imposible de analizar**: no se incrustan claves
privadas, credenciales de usuarios ni secretos de servidor. Rust no evita el
sandbox de iOS ni garantiza por sí solo una inferencia más rápida.
