# ADR 0027 · productor macOS de presión stop-only

2026-10-26 · S4-02b, no autorización de carga o gestión MLX.

Se usa sysctlbyname con selector fijo kern.memorystatus_vm_pressure_level,
entero32 bits y buffer de tamaño fijo. No shell, PID externo, comando generado,
temporales o procesos auxiliares. Alternativa: reutilizar sysctl por subprocess
del recolector; no elegida para evitar cleanup de un Child de sondeo adicional.

El timestamp procede del mismo MemoryGuard que consume el supervisor, antes de
consultar el SO. No tomar un timestamp nuevo para rejuvenecer una lectura lenta;
guardia verifica freshness/regresión/gaps al consumir. Una consulta fallida/tamaño
inesperado/valor desconocido detiene, sin fallback o rearmado. La llamada sysctl
es síncrona y breve por contrato del SO; no se puede cancelar una llamada kernel
bloqueada. No prometer plazo duro preemptivo para ese syscall.

Binding opt-in únicamente al helper auditado leaf: revalidar identidad/árbol
antes y después de leer presión. Lease original conservado por supervisor hasta
reaping; error conduce al mismo stop/quarantine. Status versión4 separa
pressure_origin native_macos_sysctl de fixture inyectada v3, mantiene
runtime_origin native_fixture_child y model_inference false. No cambia health.

Pruebas normales inyectan resultado del syscall; smoke ignorado separado lee el
host sin inducir presión y solo puede parar helper propio. Esto no demuestra
presupuesto seguro, limpieza MLX/Metal ni una sesión sostenida. Para producción
falta binding de runtime/descendientes y recuperación revisada, no otro flag.
