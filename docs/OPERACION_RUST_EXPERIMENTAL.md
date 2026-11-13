# Operación y rollback · cierre del Sprint 01

Estado: **experimental, opt-in; no instalado ni promovido al servicio habitual**.
El camino de un worker Rust tiene fixtures y dos smoke tests reales cortos con
OpenCode/Gemma. No equivale a terminar la etapa de paridad de
[`ENTREGA_Y_MIGRACION.md`](ENTREGA_Y_MIGRACION.md).

## Antes de arrancar

1. Comprobar propietarios de puertos/procesos y acordar exclusión del runtime.
   Dos coordinadores en puertos distintos **pueden competir por el mismo MLX**;
   la cola Rust no es un lock entre procesos.
2. No detener OpenCode compartido. Si se requiere pausar Python o usar un modelo,
   pedir autorización específica y registrar el estado anterior para restaurarlo.
3. No descargar pesos ni usar el identificador remoto de un modelo como sustituto
   de una ruta local existente. No instalar paquetes o dar permisos del SO.

Inspección macOS (solo lectura; sin salida/exit 1 puede significar puerto libre):

```sh
lsof -nP -iTCP:8010 -iTCP:8012 -iTCP:8081 -sTCP:LISTEN
```

En S1-06, 8010/8081 ya estaban apagados al preflight actualizado. Se autorizó un
runtime temporal, se cerró después y **no se arrancaron los servicios ausentes**.
Este dato histórico no determina el estado actual: comprobarlo otra vez.

## Discovery Rust, sin inferencia

Desde la raíz, con dependencias ya disponibles:

```sh
cargo build -p seiya-server --release --locked
export SEIYA_RUST_API_KEY="$(openssl rand -hex 32)"
target/release/seiya-server check-config --config crates/seiya-server/config.example.json
target/release/seiya-server serve --config crates/seiya-server/config.example.json
```

La plantilla escucha en 127.0.0.1:8012 y lee la clave del entorno. No contiene
`runtime`: chat da 501, nunca conecta un modelo. No sobrescribe la configuración
Python. Puerto ocupado: aborta, no hace takeover ni busca otro puerto.
Parar con Ctrl+C en esa terminal; no usar `killall`/`pkill` por nombre.

## Chat Rust y perfil de prueba

- Preparar **una copia separada** de `crates/seiya-server/chat.example.json`,
  seleccionando pesos existentes y un runtime local autorizado y exclusivo.
  Validar y arrancar con los mismos comandos `check-config`/`serve --config PATH`.
- `check-config` y arranque no hacen inferencia ni comprueban readiness. Health
  correcto no demuestra que MLX cargue el modelo o pueda responder.
- El perfil aislado probado, permisos y comando reproducible están en
  [`PRUEBA_RUST_OPENCODE.md`](PRUEBA_RUST_OPENCODE.md). El harness arranca solo
  procesos propios; no reutiliza/pausa/restaura servicios operativos.
- No copiar ese perfil sobre `~/.config/opencode/opencode.json`. No cambiar el
  proveedor predeterminado, API key o límites globales por ejecutar la prueba.

| Límite | Valor actual |
|---|---|
| Listener / runtime | Loopback; endpoint runtime canónico `http://127.0.0.1:PUERTO/v1` |
| Backend | MLX, un worker explícito; sin consenso/Ollama Rust |
| Entrada | 256 KiB, 1–100 mensajes, hasta 64 tools |
| HTTP | Content-Length único; JSON/UTF-8; sin chunked ni compresión |
| Lectura | Hasta 8 cuerpos simultáneos; deadline 1–30000 ms, default 30000 |
| Inferencia | 1 activa + 2 esperas FIFO; espera 1–120000 ms, default 120000 |
| Runtime | Timeout 1–600 s, respuesta recibida hasta 8 MiB |
| Salida | `max_tokens` o `max_completion_tokens` 1–4096; mínimo con config; ambos deben coincidir |
| SSE | Dos frames JSON y un DONE tras validar toda la respuesta; no tokens en vivo |
| Cancelación | Sin cancelación backend confirmada; trabajo conserva lease hasta resultado |

## Parada y retorno a Python

1. Terminar la prueba de su cliente privado; **no cerrar el servicio compartido**.
2. Ctrl+C en el Rust propio (o SIGTERM solo al PID verificado). Deja de admitir,
   rechaza las esperas y drena el trabajo activo, aunque el consumidor desaparezca.
3. Un error de transporte puede dejar backend incierto: no tratar salida del
   servidor o desconexión como confirmación de GPU libre. Verificar backend idle
   con su propietario antes de recrear adaptador o cederlo a Python. No reintentar
   generaciones ni matar un runtime ajeno para desbloquearlo.
4. Si el runtime es temporal y exclusivamente propio, cerrarlo después del drenaje
   según la prueba autorizada. Conservar logs/sesión sintéticos; no borrar pesos,
   chats, backups o configuración para hacer rollback.
5. El proveedor habitual y la configuración Python no fueron migrados: basta con
   dejar de usar el perfil Rust. Si Python ya estaba activo, no crear otro servidor.
   Si estaba parado, **solo arrancarlo cuando el usuario lo pida**, con su runtime
   validado/disponible, puertos libres y el mismo entorno de autenticación.

Comando manual de recuperación macOS desde la raíz, **no ejecutado en el cierre**:

```sh
cd swarm-app
/opt/homebrew/bin/python3 -m seiya serve \
  --config "$HOME/Library/Application Support/Seiya/config.json" --port 8010
```

No usar `init`, copiar la plantilla Rust sobre ese archivo ni cambiar sus modelos.
Python usa `SEIYA_API_KEY` si está presente; conservar su valor habitual sin
imprimirlo ni generar otro para el perfil existente. El comando no arranca MLX:
ese arranque requiere coordinación aparte. No garantiza restauración de un
runtime fallido, instalación limpia, o recuperación tras pérdida de datos.

La UI/historial Python siguen en `swarm-app/` y el almacenamiento del navegador;
OpenCode gestiona sus propias sesiones. Rust no migró esos datos. El ensayo de
rollback S1-07 usa configuraciones sintéticas/puertos efímeros sin acceder a ellos.

Ensayo ejecutado S1-07: CLI Python con configuración sintética, health/modelos/UI
antes y después de parar un Rust discovery propio. Mismo PID Python y mismo hash
del HTML; **cero llamadas al backend**. Rust SIGTERM 0, Python SIGINT 0 y puertos
propios cerrados al terminar. Evidencia: `seiya-s1-07-rollback.json` en el temporal
OpenCode aprobado. Es prueba de convivencia y retorno al prototipo, no de
restauración de una instalación, navegador, base OpenCode o inferencia tras fallo.

## Diferencias, pendientes y decisión de continuidad

| Área | Estado / límite al cerrar |
|---|---|
| Tools estructuradas, historial, uso, SSE | Fixtures + dos pruebas OpenCode/Gemma; cliente autoriza/ejecuta |
| Opciones de salida | Resuelto en S2-01: ambos aliases validados, conflicto 400 y mínimo con config; prueba sintética, no real nueva |
| Otras opciones OpenAI | S2-02: store solo false; stream_options/include_usage explícito. Campos raíz desconocidos y cuatro opciones antiguas no soportadas dan 400 |
| Validación | Rust más estricto en tipos/finish/usage/HTTP; no valida todos los argumentos contra cada JSON Schema ni toda relación del historial tool |
| Runtime incierto | Cuarentena 503 y cierre con fallo; recuperación manual, no endpoint de reset |
| Consenso, Ollama, web/UI/historial | Python conserva sus capacidades; no portadas al servidor Rust |
| Sesiones largas/compactación | Rust solo probado hasta 1940 tokens de prompt en el caso real; 32K no validado |
| Perfil OpenCode completo | Agente build, plugins y permisos interactivos no probados con Rust |
| Memoria | RSS muestreado parcial, sin Metal/VRAM/pico real ni comparación A/B con Python |
| Portabilidad/empaquetado | Build macOS; MSRV exacto, Windows/iOS y despliegue Rust no probados; instaladores existentes son Python |
| Protocolo nativo / herramientas SO | Especificados, no disponibles; no confundir con esta API OpenAI |

**Decisión:** siguiente prioridad, corregir presupuestos/opciones del camino
OpenCode antes de probes de memoria. La alternativa de medir primero mezcla
presupuestos distintos y puede producir comparaciones engañosas. Después se
prepara baseline con mismos pesos/contexto/salida y métricas explícitas. No se
activa idle eviction ni se porta el motor Colibri en este cierre.

El Sprint 01 cierra el vertical acotado, no toda la migración. Siguiente plan:
Incremento de presupuestos ([hoja de ruta](../ROADMAP.md)): S2-01 implementada
y probada con mocks/router; S2-02 añade política explícita de opciones (ADR 0006).
S2-03 verifica la combinación por TCP con cliente/backend sintéticos, no modelo
real nuevo. S2-04 especifica la baseline en [BASELINE_MEMORIA.md](BASELINE_MEMORIA.md):
hay un framework runner simulado, pero no driver/launcher de campaña A/B real
ni gestor adaptativo implementado. Integración real pendiente S3-04b; no medir
ni cargar modelos por completar la simulación.
No implica autorización para instalación, release, commit/push o cambio operativo.
