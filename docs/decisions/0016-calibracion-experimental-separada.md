# ADR 0016 · preparar calibración experimental opt-in

2026-10-20. Alcance: **preparar** esta política, no ejecutar Gemma con
ella. La ruta normal y su gate2 GiB siguen intactos.

- Ruta separada de calibración, un solo brazo Rust y un solo runtime nuevo:
  listener-ready (2 s), una petición de texto con salida≤256 y shutdown.
  No ABBA, tools, idle/resume, retries ni promoción automática a baseline.
- Aprobación propia, no intercambiable con ModelLoadApproval, ligada a receta,
  política y caducidad. No deserializable de datos de modelo/HTTP. Preparar no
  inicia procesos; ejecutar requiere confirmación posterior explícita.
- Política acota tiempo desde antes de spawn (30–300 s) y exige ventana de3
  observaciones normales/frescas, memoria física consistente, clearance externo,
  lease y puertos reservados. No requiere2 GiB libres **solo en esta ruta**.
  Esta excepción es un riesgo experimental declarado, no headroom disponible.
- Watchdog stop-only obligatorio durante espera/petición; error de presión/probe,
  caducidad, cancelación o plazo abortan a cleanup de Child originales. No
  garantía hard-real-time ni prevención de OOM entre muestras. Descendientes sin
  handle impiden confirmar cierre. Sin señales a procesos ajenos.
- Hashes/versiones completos antes de medir y artefactos mode model-calibration
  o calibration-rehearsal. Manifest conserva plan de referencia y detalla las
  fases/arm/repeticiones efectivas de calibración para no fingir una baseline.
  Report completed significa una calibración completada, no comparación A/B.

Alternativas rechazadas: relajar el gate normal, aceptar aprobación antigua,
ejecutar12 runs por reutilizar launch_campaign, confundir presión normal con
garantía de que el modelo cabe. Rehearsal debe demostrar que páginas libres bajas
siguen bloqueando campaña normal y solo la ruta experimental explícita las admite.
Tests solo con assets/runtime sintéticos y presión inyectada. No cambia wire Seiya,
config operativa, pesos, KV/cuántización o permisos. El presupuesto real continúa
incompleto; esta ruta permitirá medirlo con autorización futura, no inventarlo.
