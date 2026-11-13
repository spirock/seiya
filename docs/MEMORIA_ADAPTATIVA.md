# Memoria adaptativa y piezas recuperables de Colibri

**Parte del contrato Seiya `1.0-draft.1` · 2026-10-19.** Especificación futura:
no hay gestor de memoria adaptativo nuevo ejecutándose ni código de Colibri
restaurado al árbol activo. El prototipo conserva sus límites existentes.

S2-04 concreta el [diseño de baseline](BASELINE_MEMORIA.md) y sus invariantes
offline. No es una campaña medida, recolector implementado o ahorro demostrado.

## 1. Objetivo y límites físicos

Reducir memoria residente innecesaria sin corromper el estado, ocultar cambios
de comportamiento ni degradar la latencia de forma no informada. El objetivo
principal es el Mac de 16 GB; los presupuestos se medirán por dispositivo.

- Apple Silicon comparte memoria entre CPU/GPU. No sumar métricas solapadas ni
  presentar offload a CPU como ahorro automático de RAM.
- Menos parámetros activos por token no significa que los demás pesos sean
  descargables. Comprobar arquitectura y soporte concreto del runtime.
- La configuración inspeccionada de Gemma E4B tiene `num_experts: null`, atención
  sliding/full y embeddings por capa. No tratarlo como MoE ni prometer cargar
  expertos desde SSD para ese checkpoint.
- Cuantización, pesos, KV, caché del allocator, activaciones temporales y buffers
  multimodales son recursos diferentes. Limitar uno no limita todos los demás.
- Un contexto máximo anunciado no debe ocasionar reserva completa anticipada
  salvo que el runtime lo requiera; anunciar esa limitación si es el caso.
- `mmap` no garantiza que los pesos de GPU se lean del SSD bajo demanda. Mapear
  archivos, residente físico y buffers Metal son conceptos diferentes.
- Swap acumulado por sí solo no demuestra presión actual. No desactivar swap,
  usar `purge` global ni modificar parámetros del SO para aparentar una mejora.

## 2. Selección de componentes del archivo

La fuente recuperable está en el backup identificado en `docs/LIMPIEZA.md`
del repositorio (nota local de limpieza, no incluida en los instaladores).
La tabla recoge **candidatos auditados por nombre/interfaz**, no dependencias ya
integradas ni una validación completa de su corrección o portabilidad.

| Fuente archivada | Qué estudiar/reutilizar | Adaptación Seiya | Qué no trasladar |
|---|---|---|---|
| `c/resource_plan.py` | Separar planificación y detección de límites | Presupuestos Rust y probes nativos del host/runtime | Detección Linux/cgroups como si funcionase en macOS |
| `c/serve_budget.h` | Techos explícitos de generación | Admisión y reserva de salida/contexto; no generar hasta el máximo por obligación | Suponer que ese techo es un límite de RAM |
| `c/kv_prefix.h` | Reutilización e invalidación segura del prefijo | Contrato de identidad de caché y liberación con el runtime | Su layout KV o acceso directo a buffers de MLX no documentados |
| `c/expert_store.h` | Caché acotada, leases, hits/misses, resident bytes | Propiedad y lifetime de recursos; futuro store MoE compatible | Conectar su formato de pesos directamente a Gemma/MLX |
| `c/backend_metal.h` | Handles persistentes, contabilidad y residencia | Interfaz de estadísticas/capacidades de Metal/runtime | Copiar kernels, formatos cuantizados o allocator sin pruebas |
| `c/telemetry.h` | Medir residencia, caché y hardware | Eventos/estadísticas mínimas del gestor | Dashboard, trazas de prompts o protocolo antiguo completo |

Se mantiene MLX/Metal u otro runtime probado para ejecutar el modelo. Rust
coordina recursos; no reemplaza automáticamente las operaciones de inferencia.
Reutilizar una idea y portar código son decisiones distintas, registradas por pieza.

## 3. Procedencia verificable

SHA-256 de los archivos inspeccionados dentro del backup:

```text
c/resource_plan.py  a72136d7d71099870a6ff471007aa12a86639b4d1f7b8daa51f379a305d1b85a
c/serve_budget.h    2454548fae36ade99f783c32f3888308104548ded82ad7259e096591c56b1dd3
c/kv_prefix.h       6d09893b5d2d4fd366bfea5cceebd90155d079545180db8b69a27ccc6a6059e4
c/expert_store.h    e2958ab832cce953448bc458dc6a76df6abb93abf0bfe3ea80fbb99640031f8f
c/backend_metal.h   b2a7c1dba3536101e7d428c93867bbd6da83b9e175bc41300317d252889baa3f
c/telemetry.h       fd9cadbb9e3954c27fda7599757d97a5aada4cdb33a14a8d1eeb158debc18a05
```

Antes de incorporar cualquier fragmento: verificar hash y contexto de origen,
dependencias, licencia específica, avisos y autores. Conservar atribuciones,
documentar modificaciones y añadir tests de equivalencia. Revisar también las
licencias de terceros indicadas en el archivo original; el proyecto de origen
no convierte automáticamente todo su contenido en una única licencia.

No restaurar carpetas completas ni formatos antiguos por comodidad. Una pieza
elegida entra en un módulo Seiya delimitado con procedencia y prueba propia.

## 4. Contrato del gestor Rust

Operaciones objetivo, todavía no implementadas:

- `probe_resources()`: presión del SO, footprint de procesos propios y métricas
  del runtime con timestamp, unidad, alcance y disponibilidad de cada medida.
- `plan_request()`: estimar pesos, KV, modalidades y margen para el contexto
  efectivo; comprobar presupuesto antes de cargar o ampliar recursos.
- `acquire_model_lease()` / `release_model_lease()`: propiedad exclusiva del
  recurso de inferencia y recuento de usuarios; prohibir descarga con lease activo.
- `trim_caches()`: liberar solo cachés descartables mediante APIs soportadas;
  indicar qué se liberó, qué se conserva y qué métrica lo acredita.
- `evict_idle_model()`: descargar un modelo sin usuarios tras el periodo de
  inactividad; no borrar pesos, configuración, mensajes ni checkpoints persistidos.
- `apply_pressure_policy()`: decisiones escalonadas, con histéresis y enfriamiento
  para evitar ciclos de carga/descarga ante pequeñas fluctuaciones.

Separar política de mecanismo: macOS/Metal, MLX/Ollama y futuros adaptadores iOS
pueden ofrecer controles distintos. Una API ausente se declara `unsupported`;
no simular una descarga exitosa por haber llamado a un recolector de basura.

## 5. Política inicial propuesta

1. Un modelo residente a la vez en el perfil conservador del Mac. No cargar el
   siguiente hasta confirmar que la generación anterior terminó y el adaptador
   liberó sus referencias. Si la descarga falla, detener el cambio.
2. Caché de prefijos y del allocator con límites medidos separados del KV activo.
   Reutilizar solo con el mismo modelo, versión de pesos, tokenizer, chat template,
   modalidad y prefijo compatible. Invalidar si cambia cualquiera.
3. Modelo caliente durante una ventana de inactividad configurable. Descargarlo
   después y mostrar que la próxima petición tendrá arranque en frío.
4. Visión/audio: cargar encoders/buffers solo al necesitarlos **si el runtime
   admite esa separación**; si son inseparables, presupuestar el modelo completo
   o seleccionar otro adaptador con consentimiento. No atribuir ahorros ficticios.
5. Ante presión: reducir cachés descartables; no admitir trabajo adicional si
   no cabe; ofrecer menor contexto/otro modo explícitamente; cancelar de forma
   segura si el usuario lo decide. No truncar mensajes o bajar calidad a escondidas.
6. No cambiar pesos/cuántización/modelo durante un run como supuesto ajuste
   transparente. Una alternativa necesita capability check y elección informada.
7. No matar runtimes compartidos o procesos ajenos. Solo reciclar un proceso
   gestionado exclusivamente por Seiya y sin peticiones, con política publicada.
8. En iOS, obedecer avisos de presión y suspensión; no ofrecer un daemon ni el
   mecanismo desktop completo. El perfil móvil exige mediciones separadas.

Parámetros futuros: presupuesto soft/hard de memoria propia, margen para el SO,
máximo KV/cachés cuando sean controlables, idle timeout y cooldown. No se fijan
valores de RAM, porcentajes o tiempos como seguros sin medir el dispositivo.
El umbral hard es una política de admisión, no una garantía de que el SO evitará
una terminación por memoria.

## 6. Estados y observabilidad

Estados de residencia: `unloaded → loading → ready → in_use → idle → unloading`.
`pressure_limited` bloquea nuevas admisiones; `failed` conserva causa y recursos
pendientes de liberar. Una petición en cola no equivale a un modelo cargado.

La UI y los diagnósticos mostrarán modelo/modo efectivo, cachés conocidas,
presión del SO, memoria propia medida, método de medida, espera/carga/primer
token, tiempo de descarga y motivo de una decisión. Medidas no disponibles se
marcan como tales; no sumar footprint y memoria Metal si representan lo mismo.

Eventos futuros del protocolo nativo: `memory.pressure_changed`,
`memory.plan_rejected`, `cache.trimmed`, `model.loading`, `model.ready`,
`model.unloaded`. IDs de modelo/run y datos de recursos, no texto del usuario.
No se añaden eventos ficticios al SSE bufferizado del prototipo.

## 7. MoE y SSD: etapa posterior, condicionada

### Prioridad de experimentos tras la baseline

**Aplazado por indicación del usuario:** terminar primero las tareas pendientes
del sprint de recolector/driver y su puerta de campaña. No comenzar ahora la
implementación ni experimentación de KV cuantizada, MoE o carga dinámica.

La reducción buscada es también la del **runtime/modelo**, no solo la del
coordinador Python/Rust. La primera candidata de evaluación es **KV cache
cuantizada**, además de cachés acotadas y reutilización de prefijos. Verificar
soporte por versión MLX/modelo y ruta de atención: configurar un prompt-cache
pequeño no cuantiza por sí mismo el KV activo. Comparar memoria, latencia fría/
caliente, contexto y exactitud de tools frente a KV de referencia; no cambiar
bits ni política en mitad de la baseline Python/Rust para aparentar ahorro.

La combinación posterior candidata es MLX para cómputo más gestión propia de
residencia/caché/prefetch de expertos **de un MoE compatible**. No implica crear
un motor completo desde cero ni copiar todo Colibri. El routing de un MoE elige
expertos por token, pero no garantiza que sus pesos inactivos estén fuera de RAM.
Poda/dispersión arbitraria de neuronas de un modelo no MoE exige otra investigación
de calidad y kernels; no se anuncia como optimización disponible del Gemma actual.

KV cuantizada y MoE/SSD siguen siendo candidatos, **no implementados ni medidos**.
No se descargan otros modelos ni se activan políticas al añadir esta prioridad.

- Evaluar un store de expertos bajo demanda únicamente para arquitecturas MoE,
  formatos y rutas de cómputo compatibles con el runtime elegido.
- Cache acotada, leases que impidan expulsar recursos en uso, prefetch limitado
  y registro de hit rate/bytes SSD/latencia. Nada de prefetch ilimitado.
- Comparar resultados numéricos y calidad de tools frente a la referencia;
  detectar errores de almacenamiento, falta de espacio y descarga incompleta.
- Medir latencia fría y caliente, degradación sostenida, I/O y coste energético.
  Leer pesos por token desde SSD puede ahorrar residencia y empeorar mucho el uso.
- No afirmar que un MoE grande cabe o es rápido por el número de parámetros
  activos. La prueba debe incluir todos los pesos/buffers realmente necesarios.
- No es prerrequisito para la primera versión móvil ni para optimizar el Gemma
  actual. No se incluye en el runtime activo mediante este documento.

## 8. Criterios de aceptación

- Baseline reproducible y mismas tareas/modelo/contexto antes/después. Medir
  procesos y cachés durante carga, generación, tools, inactividad y recarga.
- Tras idle eviction, la memoria propia residente debe disminuir de forma
  observable o el adaptador no puede anunciar esa capacidad. No exigir que
  macOS reduzca inmediatamente el archivo de swap o sus cachés de disco.
- El usuario conoce el coste de recarga; comparar P50/P95 y no solo el mejor caso.
- Respuestas y tool calls siguen siendo correctos tras trim/recarga, con tests
  de prefijos incompatibles, cambio de modelo, cancelación y presión simulada.
- Ninguna descarga con inferencia activa ni generación huérfana al cancelar.
- Sin pérdida de chats/adjuntos, reducción oculta de contexto o filtración de datos.
- Fallos de descarga o controles no soportados generan estados reales y no OOM
  provocado cargando otro modelo. Pruebas reales en macOS; iOS se valida aparte.

Solo después de esos resultados se decide qué código portar y qué optimización
habilitar por defecto. Rust compilado no garantiza ahorro de RAM de los pesos
ni convierte el binario en secreto.
