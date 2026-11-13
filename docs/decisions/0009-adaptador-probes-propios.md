# ADR 0009 · probes opt-in de un árbol de procesos propio

2026-10-19 · S3-03. No endpoints, daemon o integración automática con Seiya server.

## Propiedad e identidad

No aceptar PIDs libres. Para un proceso externo la entrada es un préstamo
mutable de `std::process::Child` creado por el launcher. El handle original se
comprueba con try_wait; el adaptador no mata ni toma control del runtime. Si la
raíz termina, se interrumpe el muestreo, sin asumir que todos sus hijos terminaron.

La integración S3-04 añade `for_current_collector`: usa exclusivamente
`std::process::id()` y rol collector, nunca un PID pasado por el cliente. Permite
contar el overhead del propio recolector con la misma verificación de metadata.
Si se detecta que el proceso actual ya no coincide, falla cerrado. No concede
acceso a otros procesos fuera del árbol creado por el launcher.

macOS: `proc_pidinfo(PROC_PIDTBSDINFO)` obtiene PID/PPID e inicio sec/usec; no
argumentos ni entorno. Se usan bindings libc existentes en caché, no layouts FFI
inventados. Identidad comprobada antes/después de cada RSS y al recorrer padres.
El handle original evita adoptar un PID raíz reutilizado. El inicio nativo con
microsegundos distingue descendientes; no usar lstart textual con segundos.
No es un identificador criptográfico ni una operación atómica de RSS/identidad:
si el proceso termina/cambia durante la lectura, se descarta o falla, no se acepta
la memoria de otro proceso. Los datos vienen del SO, no del modelo.

Descubrimiento limitado con `proc_listchildpids` únicamente sobre nodos propios,
PPID del hijo y la identidad del padre comprobados antes/después. Máximo 32 nodos
en todo el run. Se retienen identidades/parentesco, no nombres, argumentos o
entornos. Un PID registrado no se readopta con otra identidad. Hijos reciben rol
genérico descendant; el runner no puede llamarlos servidor OpenCode sin evidencia.

Alternativas: ps global escanea procesos ajenos; lstart no basta como identidad;
aceptar un PID/parent aportado por el cliente no concede ownership. APIs privadas
de unique-id no se asumen disponibles. En otra plataforma, unsupported.

## Ejecución y cancelación

- Tres comandos fijos: `/bin/ps -p PID -o pid=,rss=`, `/usr/bin/vm_stat`,
  `/usr/sbin/sysctl vm.swapusage`. Sin shell, stdin null, stderr descartado,
  entorno limpio y locale C. No ejecutable, flags o paths desde modelo/cliente.
- Tokio process/io-util para pipes y timeout; deadline por probe 1–2000 ms,
  default 500, stdout hasta 16384 bytes, lectura de un byte extra para detectar
  exceso. No retry ni buffers ilimitados.
- El child **del probe** usa kill_on_drop: deadline, exceso o cancelación del
  future lo cierran y Tokio gestiona reaping. Nunca se señala el proceso medido.
  No hay tarea propia desacoplada que siga midiendo tras cancelar.
- Los syscalls de metadata son llamadas nativas acotadas; no se promete poder
  interrumpir un syscall del kernel. El deadline aplica al comando async, no es
  una garantía hard-real-time del host completo.
- VM/swap solo en una operación explícita separada; scope whole-host, nunca
  memoria propia ni RAM disponible. Footprint/MLX/Metal siguen unsupported.

## Interfaz y migración

Se añaden módulos al crate interno, no flags del servidor. Núcleo síncrono
permanece puro; el adaptador async entrega targets y observaciones que el runner
S3-04 podrá alimentar al núcleo/persistir, sin inventar una campaña A/B actual.
Registro de ownership y ejecutor son inyectables internamente para tests;
constructores de tablas nativas no aceptan PIDs públicos arbitrarios.

Dependencias: Tokio ya en workspace, con process/io-util; libc fijado a la versión
ya presente en Cargo.lock, solo macOS. Justificación: bindings del SDK más seguros
que copiar ABI manual y timeout/pipes sin threads bloqueantes ilimitados.

Pruebas normales: metadatos, árbol y stdout simulados, sin procesos ni comandos
reales. Smoke nativo, si se ejecuta, explícito sobre hijos propios sin modelos y
evidencia separada. Compilar no acredita campaña real, footprint o ahorro RAM.

Validación nativa básica: helper/sleep propios, RSS y VM/swap, sin modelo/red.
El primer intento detectó confusión count/bytes en proc_listchildpids; corregida
con regresión pura de count y repetición exitosa. Semántica verificada en la
[implementación pública Apple](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c),
consultada como documentación de API; no se incorpora código de esa fuente APSL.
Los campos vm_stat con guiones (File-backed/copy-on-write) se aceptan como
contadores válidos sin conservar nombres desconocidos. No es fuzzing completo,
medición de modelos o prueba prolongada de todas las carreras del SO.
