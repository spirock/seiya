# ADR 0028 · runtime propio sin fork en macOS

2026-10-26 · S4-02, experimental opt-in. No instalación o cambio del servicio activo.

## Problema y alternativas

Snapshots de descendientes/PID/health y killpg no prueban ausencia histórica de
hijos que escaparon. Registrar dinámicamente procesos sin sus Child originales
tampoco permite afirmar reaping. Se rechaza ampliar el helper leaf a MLX con un
booleano o entregar un lease al observar solo la salida de la raíz.

Para este primer adaptador se elige **impedir descendientes directos**, no un
supervisor de árboles arbitrarios: /usr/bin/sandbox-exec con perfil fijo
`(version 1)(allow default)(deny process-fork)`, luego bootstrap propio sin shell.
El bootstrap verifica que fork y posix_spawn fallan con EPERM antes de reemplazar
su imagen mediante exec por el runtime. Si la restricción no funciona, no ejecuta
el runtime. El original Child/PID/start marker persiste a través de exec.

El sandbox es solo una restricción de creación de procesos, NO sandbox de datos,
red, Metal, XPC o permisos. No protege de un runtime malicioso que pida trabajo a
brokers externos. Solo código/runtime confiable y verificado del operador; el
modelo no decide programas/rutas/args. No se permite fallback sin restricción.
No se copia código Apple: usamos herramienta del SO y headers de libproc mediante
shim C mínimo. No deps Cargo nuevas. sandbox-exec está deprecated: si desaparece
o cambia de comportamiento, fail-closed/Unsupported; no base para iOS/App Store.
No usar sandbox_check privado o ABI no declarado en el SDK.

## Interfaces y propiedad

- Nuevo launcher de librería, no endpoint de control. MLX requiere receta exacta,
  ModelLoadApproval vigente de usuario/operador con clearance/budget revisados,
  provenance nativa, ventana normal/preflight habitual2 GiB y reserva original.
  No crear esa aprobación a partir de health o del permiso de hacer desarrollo.
  Su vencimiento limita arranque y vida de la sesión. Guardia compartida añade
  stop_at_millis (solo acorta plazos, sin reset/rearmado); cada check/observe
  detiene al vencer con SupervisorStopped, aunque el siguiente probe aún no llegue.
  En este adaptador budget.manifest_sha256 debe ser el SHA256 del índice nativo
  model.safetensors.index.json; presupuesto completo, output256 y total planeado
  con margen no superior a páginas libres observadas. Es una puerta adicional
  conservadora, NO prueba de pico/capacidad. context_tokens sigue siendo plan,
  no un límite de tokens aplicado por este launcher: S4-03 debe validar/enforzar
  el perfil antes de uso real. No reutilizar una aprobación de otra receta.
  Revalidar también la caducidad5 s del preflight justo antes de spawn: metadata
  lenta no puede rejuvenecer las páginas libres con una muestra nueva de presión.
- Ensayo separado solo binarios del repositorio/runtime sintético y directorios
  de lock nuevos propios. No se ejecuta MLX para acreditar ese ensayo.
- Bootstrap acotado por pipe y arranque supervisado: presión nativa/guardia antes
  y durante carga, identidad/árbol verificados; socket TCP loopback debe pertenecer
  al Child original (libproc), no basta que responda un endpoint. No sondeo HTTP
  que pueda alcanzar otro propietario en una carrera de bind.
- El shim C usa headers del SDK para consultar FDs/socket de **ese PID propio**;
  límites fijos y tamaño exacto; no lsof/subprocess nuevo que pueda quedar huérfano.
  Build macOS usa cc/ar del SO, sin instalar paquetes ni SDK.
  libproc/sysctl son llamadas síncronas: límites de bucles/FDs/plazos no prometen
  preempción de un syscall bloqueado por el kernel.
- Una vez listo, supervisor conserva Child y NativeSessionLease. Presión/deadline/
  cancelación: SIGTERM una sola vez a identidad fuerte aún no recogida, try_wait
  verifica salida. No killpg, SIGKILL automático, PID adoptado o rearmado. Cualquier
  descendiente observado invalida cobertura de forma permanente.
- Errores/timeout/cancelación de bootstrap retienen Child+lease si no se confirma
  cleanup. Proof completo exige bootstrap restrictivo confirmado y reaping de la
  raíz; ni HTTP ni un booleano del cliente son pruebas. Conservar cuarentena y
  recuperación explícita acotada del supervisor previo; no crash durability.
- Integración HTTP solo de librería con scheduler/MemoryGuard compartidos. La CLI
  normal no empieza a poseer runtimes ni adquiere capacidades por leer JSON.
  Snapshot supervisor nuevo distingue runtime sintético vs MLX, presión nativa y
  contención process-fork; no cambiar versiones1–4 ni confundir con health1/2.
  model_inference se conserva false en JSON de fixtures1–5; para runtime MLX
  es null (no observado), no true por arrancar. Tipo Rust pasa a Option<bool>;
  consumidores internos migran comprobaciones a Some(false). Wire1–4 intacto.

## Aceptación y límites

Primero probar kernel deny fork/spawn, herencia a través de exec, socket propio,
presión/cancelación/idle y reaping/lease con helpers HTTP sintéticos, más mocks de
errores. Después pruebas de integración con scheduler/UI. Esto implementa el
binding restringido; no demuestra compatibilidad MLX/Metal, rendimiento, memoria
segura o conversación sostenida. S4-03 seguirá necesitando permiso real y todos
los gates/budget. Si MLX necesita subprocess, ese perfil es incompatible, no
ampliar permisos ni restaurar un camino sin contención para ocultarlo.
