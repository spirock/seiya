# Compatibilidad OpenAI · referencia Python Seiya 0.1.0

**S1-01, 2026-10-19.** Fixtures sintéticas del comportamiento existente usado
por OpenCode. Son diferentes del protocolo nativo propuesto `contracts/v1/`.
No son capturas de conversaciones, archivos personales o respuestas de modelos
reales. El checkpoint y el token de autenticación del test son ficticios.

## Ejecutar

Desde `swarm-app/`:

```sh
python3 -m unittest discover -s tests -p 'test_api_compat.py' -v
```

La suite arranca un HTTP server Python de referencia en loopback y puerto
efímero, con un transporte de inferencia simulado. **No conecta con el runtime
MLX/Ollama ni ejecuta archivos, comandos o herramientas del cliente.** No toca
el servicio activo del Mac, OpenCode, los pesos ni su configuración.

## Archivos y formato

- `scenarios.json`: configuración de prueba y escenarios secuenciales. Una
  conversación de tools conserva el mismo coordinador entre pasos.
- `validation.json`: peticiones inválidas, tool calls de backend inválidas,
  límites y mensajes de cola. `patch` sustituye claves del `base_request`
  superficialmente; `body` sustituye el cuerpo completo.
- `http-policy.json`: guardas de autenticación, Host, Origin y ruta desconocida.
  `headers` modifica los headers válidos del harness; `null` elimina un header.
- `rust-output-budget.json`: **extensión Rust S2-01, no golden Python**. Fija
  alias de salida, enteros 1–4096, ausencia, mínimo con config, igualdad/conflicto
  entre ambos campos y tipos inválidos. Tests Rust de adaptador/router usan el
  fichero; el prototipo Python no cambia ni se declara compatible con el alias.
- `rust-chat-options.json`: **extensión Rust S2-02**, contrato sintético de store,
  stream_options/include_usage y campos raíz desconocidos. No modifica Python,
  ni es captura de todos los valores enviados por OpenCode real.
- `rust-opencode-profile.json`: S2-03, petición sintética con los nombres de
  campos observados en S1-06. Pruebas TCP completas cliente/Rust/backend sintético
  combinan presupuesto, opciones y tool roundtrip. No es una captura literal.
- `runtime-bridge.json`: S4-02c, extensión opt-in de inferencia/estado, no golden
  Python default. Handshake1/hex-utf8-v1, configuraciones separadas sintéticas,
  health filtrado y error con código. No protocolo nativo, launcher MLX, lector
  de páginas o autorización. Tests Python/Rust y smoke conjunto con runtime falso.

Cada paso de escenario define método/ruta/cuerpo, resultado HTTP esperado y,
si hay inferencia, una `runtime_response` o `runtime_error` simulado. El harness
compara la llamada real al transporte con:

```json
{
  "url": "http://127.0.0.1:65535/v1/chat/completions",
  "timeout": 5,
  "body": {
    "model": "fixture-checkpoint",
    "messages": "exactamente los mensajes de entrada, como array, no esta cadena",
    "stream": false,
    "max_tokens": 256,
    "temperature": 0,
    "chat_template_kwargs": {"enable_thinking": false}
  }
}
```

Si la entrada incluye tools no vacías, se añade exactamente ese array. Las
sobrescrituras de `runtime_request` se aplican después. No se reenvían el modelo
público, modo de enjambre, opciones stream del cliente u otros campos internos
como si fueran opciones del runtime. La fixture de configuración limita la
salida a 256; eso **no cambia** la configuración de producción de 2048.

Un error de guardas/validación/cola no debe invocar al runtime. Los escenarios
que sí lo necesitan exigen el número exacto de llamadas y consumo completo
de la respuesta preparada. No se usan snapshots regenerados automáticamente.

## Comparación y campos variables

Se compara el JSON completo semánticamente: claves, valores, arrays, nulls y
campos adicionales. El orden de claves y whitespace del JSON HTTP no importan.
La comparación canónica distingue booleanos, enteros y números decimales;
no usa la igualdad Python que trataría `true` como `1`.
El string `function.arguments` se conserva **literalmente**, sin convertirlo a
objeto ni reordenar su JSON. IDs de tool call e historial no se normalizan.

Solo se normalizan estos campos tras comprobar su tipo/forma:

| Campo | Comprobación | Marcador |
|---|---|---|
| `id` de completion/chunk | `chatcmpl-` + 32 hex minúsculas | `<completion-id>` |
| `created` | entero positivo, no booleano | `<unix-seconds>` |
| `swarm.elapsed_seconds` | número finito no negativo | `<elapsed-seconds>` |

SSE es **bufferizado**: dos frames JSON y uno `[DONE]`. Los chunks conservan
el mismo ID y timestamp, `index` de cada tool call, `finish_reason`, uso y
metadatos. Se comprueba `Content-Type`, no-store, cierre y terminador. No se
afirma que existan deltas en vivo. Truncación o fallo de inferencia antes del
resultado se devuelven como JSON de error, incluso si se pidió `stream:true`.

Se comprueba `Content-Type` JSON y no-store. Headers de infraestructura como
`Date`, `Server` o el cálculo de Content-Length no se congelan como ABI.

## Límites y concurrencia generados por el harness

Para no almacenar payloads repetidos gigantes, la suite genera desde `limits`:

- 1/100 mensajes válidos y 101 rechazados.
- 64 tools con nombres únicos válidas y 65 rechazadas.
- Extremos válidos de tokens y temperatura; valores fuera de rango y booleanos
  inválidos fijados en las fixtures.
- Cuerpo JSON exactamente de 262144 bytes, con padding ASCII, y tamaño declarado
  262145 rechazado antes de leer/enviar el payload para evitar carreras de socket.
- Cuerpo vacío, UTF-8 inválido, JSON roto y Transfer-Encoding no soportado.

Los errores de parser JSON/Unicode solo fijan HTTP 400 y `error.message` string;
no se congela el texto del parser de una versión de Python.

La concurrencia se comprueba con eventos, no con un modelo lento real:

1. Bloquear el transporte simulado de la primera petición.
2. Enviar una segunda desde otro cliente HTTP y comprobar que espera.
3. Consultar health y verificar `busy:true` durante esa espera.
4. Liberar la primera y exigir dos HTTP 200 y una sola inferencia simultánea.
5. Por separado, saturar los slots para comprobar 429 por cola llena y por timeout,
   liberar capacidad y confirmar que otra petición vuelve a funcionar.

El tiempo de espera de tests es corto y explícito; no cambia los 120 s del
servicio. El futuro harness Rust debe reproducir esas condiciones, no depender
de tests que tengan acceso a internals del coordinador Python.

## Requisitos congelados

- `/v1/models` solo anuncia `pegassus`, propiedad de `seiya`.
- Fast y tools usan un worker. El backend siempre recibe `stream:false`.
- Historial system/user/assistant/tool válido se preserva. Assistant puede tener
  `content:null` y llamadas estructuradas; resultados llevan `tool_call_id`.
- Se reenvían definiciones/tools, no se ejecutan en Seiya.
- JSON escrito como prosa/Markdown sigue siendo texto; no se «repara» ni ejecuta.
- Uso ausente/null produce contadores cero. Respuestas truncadas, usage no objeto,
  estructura inválida, tools no ofertadas, argumentos/IDs inválidos fallan.
- Opciones OpenAI actualmente no admitidas se rechazan de forma explícita.
- El techo de salida limita la petición al runtime; no se trunca una respuesta
  completa ni se convierte una generación cortada en éxito.

## Alcance y diferencias aceptadas para el MVP Rust

S2-01 añade `max_completion_tokens` como alternativa a `max_tokens`. Ambos
iguales son válidos, diferentes dan 400 antes de generación; no hay precedencia
silenciosa. Presupuesto efectivo = mínimo de cliente y config, enviado como
`max_tokens` al backend. Sin ambos se usa config. Todos los valores presentes
deben ser enteros 1–4096, incluidos cuando config es menor. Decisión/migración:
`docs/decisions/0005-presupuesto-salida-openai.md`. Las golden originales y el
comportamiento Python se conservan.

S2-02 (ADR 0006) acepta `store:false` o ausencia, sin reenviarlo al backend ni
ofrecer persistencia Seiya. No controla sesiones OpenCode/logs del runtime.
`stream_options` requiere stream true y un objeto con solo `include_usage`
booleano opcional: default true conserva usage final; false lo omite de SSE.
JSON no stream conserva usage. Tipos inválidos, store true y otros campos raíz
no soportados dan 400 antes de admisión/generación, sin reflejar datos privados.
Se mantiene el framing histórico de dos chunks y DONE; no se afirma soporte de
todos los detalles del streaming OpenAI ni validación completa de campos anidados.

1. **No es paridad total:** UI, búsqueda web, `/api/chat`, persistencia de chats,
   visión, modo consenso y compatibilidad completa de Ollama quedan fuera de
   este set. Tests existentes de Python siguen cubriendo otras rutas.
2. El MVP Rust debe rechazar el consenso no implementado. No debe devolver una
   respuesta fast mientras afirma haber hecho consenso. La prueba de OpenCode
   se configurará con `swarm_mode: fast` explícito.
3. La configuración sintética usa un backend de la forma MLX chat completions.
   El unload específico Ollama y fallos de cambio de modelo siguen en los tests
   Python; necesitan fixtures adicionales antes de declarar paridad de ese backend.
4. El prototipo no valida a fondo todos los mensajes de herramientas aportados
   por el cliente ni el esquema de argumentos particular de cada tool. El cliente
   externo conserva esa responsabilidad. No convertir estas fixtures en una
   certificación de seguridad o compatibilidad OpenAI completa.
5. No se obliga a reproducir aceptaciones accidentales de valores malformados,
   campos desconocidos ignorados, JSON no estándar o errores internos no
   documentados. Una validación más estricta en Rust debe declararse y probarse
   antes de cambiar el cliente, no introducirse como regresión silenciosa.
6. No se congela la falta de validación del Content-Type de entrada como un
   requisito. Si Rust exige JSON, documentarlo en el perfil experimental.
7. Los tests HTTP completos siguen siendo de la referencia Python. Rust compara
   discovery/guardas y, desde S1-04, escenarios HTTP de chat JSON no stream y
   tools con los mismos JSON y transporte simulado. S1-05 añade fixtures SSE
   bufferizadas y cliente TCP simulado con resultados tool sintéticos.
   S1-06 valida dos smoke tests reales OpenCode/Gemma en un perfil mínimo aislado
   (ver `docs/PRUEBA_RUST_OPENCODE.md`); no prueba sesiones largas ni todo OpenCode.
   Las fixtures solas no validan un modelo; el smoke real tampoco acredita
   paridad HTTP completa ni todos los modelos.

Dos tests Rust leen y comprueban la estructura común de estas fixtures y su
separación del protocolo nativo. No se presentan como una suite de servidor.

Cambios deliberados: nueva revisión de fixtures y nota de migración. No actualizar
los resultados esperados para esconder una regresión. Mantener esta revisión como
referencia reproducible al introducir otra implementación.
