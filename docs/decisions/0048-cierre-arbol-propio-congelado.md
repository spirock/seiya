# ADR0048 · cleanup de árbol propio y bloqueo crítico

Intento04 con permiso warning10s/swap2GiB: Gemma devuelve SEIYA_OK; al cargar Qwen
se observa presión crítica y se para según permiso. Cleanup devuelve
OwnershipChanged; fallback Drop señaliza solo identidades previamente conocidas.
Revisión posterior read-only: raíz/grupo48932 ausentes, puerto56425 sin listener,
presión normal. Se conserva marcador y evidencia: no reinterpretar como cleanup
completo confirmado ni borrar cuarentena para volver a cargar.

Hipótesis de código: mientras close enumera y termina hijos antes de raíz, el
padre puede crear/reemplazar un worker y hacer fallar la prueba de grupo cerrado.
También puede ser un cambio real de ownership; sin traza de identidades por fase
no se atribuye causa definitiva. No adoptar por nombre/PID ni signalizar todo un
grupo que contenga un miembro no verificado.

Corrección interna de OwnedChild::close, sin nuevo constructor de PID externo:
SIGSTOP solo a raíz/descendientes con inicio/grupo verificados, descubrir desde
padres comprobados y pausar nuevos nodos en rondas acotadas. Exigir árbol estable
y grupo íntegramente conocido antes de SIGKILL a cada identidad propia (no killpg).
Padres pausados no pueden reemplazar workers durante el cierre. Fallo conserva
cuarentena y fallback solo sobre conocidos; PID/start siguen sin pidfd en macOS.
No garantiza cleanup si hay escape o identidad cambiada: eso sigue siendo error.

Alternativa TERM de hijos antes de raíz permite respawn; TERM solo de raíz puede
perder descendientes no observados; killpg por número arriesga ajenos. El cierre
forzado de este runtime efímero no modifica store fuente ni proyectos del usuario.
No se espera flush de caches privadas de inferencia como criterio de éxito.

Verificación con fixtures Rust propias, sin modelos: listener real propio/ajeno,
árbol parent+worker que intenta reponer el hijo al terminar, grupo vacío y puertos
liberados. Mantener tests de PID reciclado/grupo ajeno. No ejecutar otro ensayo
LLM para reproducir el fallo ni modificar stop por critical. La prueba04 y el
objetivo web real siguen bloqueados por recursos/revisión; marcador original
preservado hasta intervención explícita y controles pertinentes.
