# ADR 0007 · núcleo de observaciones de procesos

2026-10-19 · S3-01. Interfaces internas nuevas, sin endpoints públicos.

## Elección y alternativas

- Dentro del servidor: acopla medición al listener y puede activar probes al servir.
- En Python: rápido para el harness pero duplica la nueva lógica compartida.
- **Librería Rust `seiya-observe`:** núcleo puro y proveedor inyectable, separada
  de contratos wire nativos y de inferencia. Sin dependencias nuevas salvo Serde
  ya usada en el workspace. No incluye ejecutable, timer, shell, red o filesystem.

## Identidad y ownership

El futuro launcher registra targets creados por él con PID, marcador de inicio
opaco y rol. Esos datos son configuración confiable, nunca argumentos del modelo.
La librería **no prueba por sí sola que el proceso sea propio**. El mecanismo de
registro y lectura real es responsabilidad del adaptador S3-03; no exponer un
endpoint que acepte PIDs arbitrarios como si eso concediera permiso.

Proveedor devuelve identidad observada + RSS KiB, exited o error redactado. PID
o inicio distintos del target son fallo terminal: no reutilizar ese PID ni tomar
su memoria como propia. Exited es irreversible en esta colección y no se vuelve
a consultar; errores transitorios conservan target activo y pueden recuperarse.

## Cotas y muestras

- 1–32 targets, PIDs únicos positivos, marcadores de inicio no vacíos/acotados.
- Máximo configurable 1–100000 snapshots; el núcleo no conserva historial:
  devuelve uno y el caller lo persiste/descarta. Nada de acumulación sin límite.
- Timestamp en milisegundos monotónicos desde el inicio del run, aportado por el
  caller/clock del futuro runner. Regresión temporal falla terminalmente antes
  de consultar el proveedor. Fases tipadas, no texto libre ni prompts.
- RSS KiB → bytes con overflow comprobado. Un valor 0 observado es válido; falta
  de muestra no se convierte en cero. No calcular una suma física de procesos.
- Estado unavailable con razón tipada para error/exit/overflow; footprint, MLX y
  Metal unsupported con null explícito. No presentar RSS como footprint/GPU.

## Migración y alcance

No integra probes en Python/Rust server, no migra configuración y no cambia el
protocolo nativo. Serialización diagnóstica interna marcada v1, no API estable
de sesión. Tests usan PIDs/valores sintéticos y proveedor en memoria, nunca ps,
modelos, procesos ajenos, correo o comandos del SO. Parsers S3-02 y adaptador
S3-03 se validan aparte antes de anunciar colección real.
