# ADR 0015 · presupuesto explícito y vigilancia durante carga

2026-10-20 · incremento S3-05, sin cargar modelos. No reemplaza ADR0014 ni el
gate actual. Implementar componentes no valida un presupuesto de Gemma.

## Decisión

- Presupuesto puro de categorías planificadas no solapadas: pesos residentes,
  KV activo, temporales/activaciones, cachés descartables, overhead runtime y
  coordinador. Margen del SO separado. Cada importe tiene procedencia y tipo
  estimado/medido; faltantes permanecen null y bloquean un total completo.
- Identidad del modelo mediante digest de manifiesto aprobado y contexto/salida
  explícitos; overflow, importes inválidos y metadatos excesivos se rechazan.
  No sumar RSS/Metal/footprint como categorías, ni usar bytes de shards como
  memoria residente. Un total estimado no es peak medido ni memoria disponible.
- `LoadMonitor` es una máquina pura, acotada y terminal tras fallo: presión
  warning/critical/unknown, muestra futura/vieja, hueco>5 s o reloj regresivo
  detienen la campaña. Presión normal no acredita capacidad ni admite carga.
- Driver de campaña añade probes de presión en espera de listeners, fronteras,
  idle y requests; cada segundo best-effort durante trabajo. Solo sysctl fijo
  de lectura sin shell. El rehearsal inyecta esas señales, no provoca presión.
- Ante fallo del monitor se registra causa/última observación y se devuelve
  error al runner; cleanup usa únicamente Child originales. Puede interrumpir
  trabajo propio por seguridad: se declara abortado, no cancelación propagada
  al modelo ni finalización correcta. No iniciar otro brazo. Descendientes sin
  handles mantienen cleanup incierto. Señales/races de kernel no son hard realtime.

## Alternativas y consecuencias

No se habilita admisión pressure-only ni se baja2 GiB. Monitorizar presión no
impone un límite duro de RAM: la presión puede subir entre muestras. El overhead
del probe existe y queda incluido en tiempos/collector; comparación futura debe
usar el mismo mecanismo en ambos brazos. No se muestrea nueva RAM global para
sumarla a free/inactive ni se cambia cuantización/KV/cache del workload.

## Migración y siguiente requisito

API de benchmark interna, esquema de receta y wire Seiya sin cambios. Informe
de presupuesto aditivo, sin knobs de carga ni valores inventados para Gemma.
Para sustituir el gate falta completar importes con evidencia para el runtime/
contexto concretos y decidir política de riesgo explícita. Tests puros y ensayos
de presión inyectada con procesos/archivos propios, nunca presión real provocada.
La autorización de una campaña anterior no autoriza ocultar esta incertidumbre.
