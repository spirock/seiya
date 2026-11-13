# ADR0034 · contadores del asignador, sin activar runtime

2026-11-11. Preparación offline autorizada; no carga/instrumentación de un
runtime compartido ni modificación del servicio operativo.
Extensión posterior autorizada: ADR0035 añade el canal opt-in propio; no cambia
las semánticas de estos contadores ni instrumenta servicios compartidos.

## Alternativas y decisión

1. Medir MLX desde el colector: rechazado, observaría otro asignador/proceso.
2. Equiparar RSS con MLX/Metal o sumar contadores: rechazado por solapamiento.
3. Adaptador Python dentro del runtime que recibe `mlx.core` **ya importado**,
   más decoder puro Rust: elegido. No importa MLX, ejecuta modelos o abre red.

Formato interno `seiya-runtime-memory/1`, una muestra JSON≤4096 bytes UTF-8,
campos cerrados. PID del proceso emisor, secuencia desde0 y tiempos monotónicos
relativos al adaptador (inicio/fin de lectura, nanosegundos); no son timestamps
del colector. La secuencia sirve para detectar huecos, no acredita autenticidad.
La comparación de PID con Child es necesaria pero insuficiente frente a reuse:
el futuro canal debe ligarse al Child original/identidad de inicio y run propio.
No hay canal, endpoint ni integración automática en este cambio.

Métricas en bytes: active/cache/peak del asignador MLX; lectura secuencial no
atómica, sin synchronize/eval, reset, clear_cache, tuning o asignaciones de
tensores. Peak es global del runtime, desde inicio o último reset **externo
desconocido**: nunca llamarlo pico de petición/fase. No comparar valores entre
contadores como si fueran una foto simultánea. Metal total y footprint son
null/no_validated_probe; RSS conserva su probe nativo separado, no se infiere.
APIs ausentes, excepciones y valores inválidos producen null con razón fija,
sin conservar mensajes de excepción ni convertir desconocido en cero.

Fuente revisada estáticamente: MLX0.32.4 core/__init__.pyi:815–857, instalado;
MLX-LM0.32.2 no se modifica. No fallback silencioso a `mx.metal` de otras
versiones. El formato no prueba semántica del dispositivo por pasar fixtures.

## Consecuencias, migración y rollback

No se alteran Snapshot/Measurement existentes ni su unsupported/null. Futuras
campañas guardarán estos reportes como evidencia separada solo después de
vincular emisor y run mediante transporte propio acotado y deadline supervisado.
Las llamadas al API pueden bloquear: no hay timeout hard en Python; deben
quedar dentro del runtime aislado, nunca del colector/watchdog stop-only.
El decoder Rust valida tamaño, tipos, razón/valor, scope, PID esperado y reloj
intra-muestra, pero no admite datos como permisos ni valida el canal ausente.
Rollback: no importar el adaptador; no hay datos previos que migrar ni servicio
que reiniciar. Sin dependencia nueva o cambio del contrato HTTP público.
