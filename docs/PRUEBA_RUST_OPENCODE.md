# S1-06 · prueba real aislada Rust / OpenCode

2026-10-19. **Dos smoke tests cortos aprobados**, no una certificación de
sesiones largas, todos los modelos, plugins o ventana 32K.

## Aislamiento y autorización

El ensayo incluía autorización acotada y pausa temporal de Python. Al volver a
verificar, Python/8010 y MLX/8081 **ya no estaban activos**, sin haberlos detenido
el ensayo. Se autorizó de forma separada arrancar un MLX
temporal usando los pesos Gemma existentes, y cerrarlo después. No se reiniciaron
los servicios ausentes ni se detuvo el servicio compartido de OpenCode.

- Runtime MLX propio en loopback y puerto libre; propiedad del listener verificada
  por PID antes de configurar Rust. Ningún coordinador comparte ese endpoint.
- Rust release propio en puerto efímero, clave Bearer temporal en entorno.
- OpenCode `run --standalone`, HOME/XDG/TMP separados, sin credenciales heredadas;
  política que deniega todos los proveedores salvo `seiya`. Sin fallback remoto.
- Perfil primario `smoke`, instrucciones cortas, máximo ocho pasos; solo lee
  `calc.py`/`test_calc.py`, edita `calc.py` y ejecuta el comando fijo
  `/opt/homebrew/bin/python3 -m unittest -v`. Sin subagentes, MCP, web ni skills.
- Datos sintéticos. El test fijo valida el AST antes de importar el módulo
  editable: admite solo la función `add(a,b)` con suma/resta, sin código arbitrario.
  El hash del test fijo no cambia. Esto **no es un sandbox general del SO**.
- Pesos locales; `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`; no instalación,
  descarga de pesos, cambio del proveedor global o permisos del sistema.

## Recorrido observado

OpenCode → proxy local de medición → Rust → MLX propio → SSE bufferizado →
OpenCode ejecuta la herramienta → resultado tool conservando ID → siguiente turno.
El proxy conserva el cuerpo y auth, reenvía al Host del listener Rust y registra
bytes, campos, IDs, argumentos, status, uso y tiempo; no transforma tool calls.

En ambas ejecuciones:

1. `read(calc.py)` muestra la resta incorrecta.
2. `read(test_calc.py)` obtiene los tests fijos y su guardia AST.
3. `edit(calc.py)` cambia una vez `return a - b` por `return a + b`.
4. `shell` ejecuta el comando permitido: **3 tests, OK, exit 0**.
5. Respuesta final sin tool calls; sesión persistida en **idle/succeeded**, CLI 0.

Los cuatro IDs/argumentos de llamadas se comparan con los eventos `tool_use`
completados y los IDs devueltos en mensajes tool. Se repiten los tres tests
independientemente: OK. Seiya no ejecuta ninguna herramienta por segunda vez.

## Resultados medidos

OpenCode **2.0.32**, MLX **0.32.4**, MLX-LM **0.32.2**, Transformers **5.20.1**.
Mac Apple Silicon, 16 GiB, checkpoint existente `mlx-gemma4-e4b` (4-bit).
Worker único, temperatura 0, salida máxima 2048, perfil OpenCode 32768/2048,
cache MLX de una entrada/128 MB, prefill 256, concurrencia decode/prompt 1.

| Medida | Run 01 | Run 02 |
|---|---:|---:|
| Tiempo total del harness, incluido arranque/cierre (s) | 14.743 | 15.084 |
| Peticiones chat/SSE, todas 200 y DONE | 5 | 5 |
| Errores HTTP / mensajes de compactación | 0 / 0 | 0 / 0 |
| Latencia por petición (s) | 4.121, .888, 1.945, 1.785, 1.795 | 4.270, .898, 1.976, 1.798, 1.812 |
| Mediana por petición (s) | 1.795 | 1.812 |
| Mayor entrada HTTP (bytes) | 8609 | 8609 |
| Mayor prompt reportado por MLX (tokens) | 1940 | 1940 |
| Suma de tokens input, **incluye historial repetido** | 8503 | 8503 |
| Suma de tokens de salida | 116 | 116 |

Memoria run 02: máximo **muestreado** con `ps RSS` cada segundo: MLX 4219664 KiB,
Rust 3984 KiB y CLI OpenCode 94640 KiB. Solo PIDs iniciados directamente:
**excluye el servidor hijo privado de OpenCode**, memoria compartida/GPU y picos
entre muestras. No es RAM total, VRAM/Metal ni evidencia de ahorro frente a Python.
Run 01 conserva muestras por PID, pero no atribución automática por rol.

La compactación se cuenta leyendo **solo la base aislada ya parada**, en modo
read-only: tipos `agent-switched`, `user`, cinco `assistant` e `idle`; no mensajes
de compactación. También se comprueba el último outcome `succeeded`. No se abre
la base del usuario. Este caso pequeño no ejercita compacción ni el contexto 32K.

OpenCode envía `max_completion_tokens`, `stream_options` y `store`. En la versión
probada en S1-06, Rust no interpretaba esos campos; el presupuesto efectivo fue el
`max_output_tokens:2048` configurado en el adaptador. No se acredita compatibilidad
de presupuestos distintos a través de ese alias ni todas las opciones OpenAI.

Actualización S2-01: Rust ya valida/interpreta el alias de presupuesto (ADR 0005),
con pruebas sintéticas de valores distintos al config. No se repitieron los
smoke tests reales aquí registrados ni se acreditó el resto de opciones.

Actualización S2-02: store false y stream_options/include_usage ya tienen una
política Rust explícita, con rechazos de desconocidos (ADR 0006), probada con
mocks/router. No transforma estos smoke tests históricos en evidencia real de
esas reglas ni acredita compatibilidad completa de OpenCode.

## Repetir, solo con autorización de carga real

Desde la raíz, con binarios y pesos **ya existentes**, elegir un directorio de
salida nuevo en el temporal aprobado de OpenCode:

```sh
cargo build -p seiya-server --release --locked
python3 local-tools-tests/seiya_rust_opencode_smoke.py --execute-real \
  --model /RUTA/ABSOLUTA/AL/CHECKPOINT_EXISTENTE \
  --mlx /RUTA/ABSOLUTA/AL/VENV/bin/mlx_lm.server \
  --output /RUTA/TEMPORAL/APROBADA/seiya-s1-06-NUEVO
```

No reutilizar un runtime compartido, arrancar modelos simultáneos si falta RAM,
ni matar procesos para liberarla. El script no pausa ni restaura servicios ajenos.
Conservar los artefactos sintéticos en el output; no borrar bases/sesiones del usuario.
El cierre señala solo sus procesos: CLI 0, Rust 0 y MLX temporal SIGTERM (-15)
en ambas ejecuciones. Listener temporal ausente después; servicio OpenCode original
seguía en su PID/puerto. No se deja otro servicio activo.

Evidencia local, bajo el temporal OpenCode autorizado:

- `seiya-s1-06-real-01/`: stdout JSON de OpenCode, requests, tests independientes,
  logs propios, paths aislados, muestras RSS y `inspected-evidence.json`.
- `seiya-s1-06-real-02/`: lo anterior y `summary.json` con validación automática
  de herramientas/resultados/idle, métricas y PIDs. La primera ejecución se verificó
  también con ese validador, offline, tras añadirlo.

Ocho tests sintéticos del validador se incluyen en `make test`, **sin inferencia
real**: rechazan éxitos basados solo en un archivo cambiado, tests fallidos,
argumentos alterados, falta de resultados, SSE incompleto, doble ejecución o
sesión sin idle. El comando real nunca forma parte de CI o `make test`.
