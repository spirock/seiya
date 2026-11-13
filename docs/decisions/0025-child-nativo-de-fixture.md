# ADR 0025 · Child nativo limitado a helper conocido

2026-10-26 · S4-02b.2. Ensayo nativo sin pesos, runtime MLX, red ni presión real.

- Añadir inspección de identidad/árbol a probes sin ejecutar RSS/VM/shell. Solo
  desde el Child original registrado; validar identidad/ancestría antes/después.
- Launcher Rust de fixture, no de modelos: binario del repositorio seleccionado
  por operador confiable, argumentos cerrados, entorno vacío, stdin/stdout propios.
  Nombre/permisos/identidad de archivo se comprueban; no autentican su procedencia.
  No pasar una ruta/orden generada por modelo ni exponer launcher por HTTP/CLI.
- Helper nuevo de protocolo fijo: sin fork/red/modelos; readiness por pipe acotado,
  EOF cierra; variante controlada ignora EOF para probar SIGTERM. Autolímite10 s
  evita un helper indefinido si falla el padre. No extrapolar a programas arbitrarios.
- Capturar PID/start marker desde Child/proc_pidinfo, nunca desde stdout. Mantener
  el handle original; no adoptar PIDs. Revalidar antes de EOF/señal y cada muestra.
  Un descendiente observado marca cobertura incompleta permanentemente. No matar
  descendientes sin handle; no devolver Complete si hubo alguno.
- Primero EOF, luego SIGTERM al Child original si sigue vivo y coincide identidad;
  try_wait recoge su salida. Nunca repetir señal a Child recogido ni enviar señal
  tras error/cambio de identidad. El lease solo se libera tras prueba aceptada.
- Bootstrap acotado y guard de preparación: ante fallo intentar cerrar/recoger el
  helper propio. Si no se confirma, retener Child+lease fail-closed, no soltarlos en
  Drop. Cancelar el bootstrap también retiene capacidades; no recuperación automática.
  Este resguardo puede retener FDs hasta salida del padre, como ADR0023.
- Supervisor de fixture nativa: Status versión3, mode native-fixture-supervisor,
  runtime_origin native_fixture_child, pressure_origin injected_fixture,
  lease_origin explícito y model_inference false. Inyección previa conserva v1/v2.
- Presión sigue siendo fixture constante. No leer host ni iniciar MLX al probar
  Child. Probar el observador real requiere otro incremento explícito/aislado.

## Límites y alternativas

Un programa arbitrario puede crear/reparentar hijos entre inspecciones. Las
instantáneas no prueban ausencia histórica; Complete solo se admite aquí por
contrato del helper auditado sin fork, más identidad/inspecciones/Child reaped.
No es contención de procesos, job/cgroup o validación de todos los runtimes.
Ampliar a MLX exige diseño de descendientes y pruebas propias, no otro flag.
No usar killpg global, PID libre, health HTTP o stdout como prueba de ownership.

Tests normales: fixtures propias y mocks del árbol; smoke nativo separado ignored,
solo binario recién construido/paths de prueba nuevos. Sin modelos/servicios
activos. No instalar, modificar permisos del SO o sustituir Python/OpenCode.
