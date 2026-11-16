# ADR0070 · criterio de admisión del ensayo local8016

Usuario cambia criterio: una intención histórica `native-model.inflight` con
`cleanup_confirmed=false` y cuarentena RootExited **no se reescribe ni se
interpreta como cleanup confirmado**. En debug `--test-chat` exclusivamente,
se puede aceptar un riesgo residual documentado tras revisión de identidad,
grupo/PID ausentes, puerto original libre, inventario compartido vacío, presión
normal y coincidencia de registros en ambos dominios. La admisión normal de8015
y release continúa rechazando los marcadores.

`stop.json` y `cleanup.json` bajo el directorio de la corrida son **resultados
históricos**: se leen para revisión, nunca se corrigen. `native-model.inflight`
y `pair-cleanup-uncertain.json` en ambos dominios son **registros históricos que
también controlan la admisión actual**. Se conservan byte por byte; el modo dev
solo exceptúa su identidad exacta tras revisión. Una nueva intención o cuarentena
en archivos separados vuelve a ser un bloqueo, también en ese modo.

Alternativas: borrar/archivar los marcadores (pierde la barrera histórica),
afirmar cleanup confirmado (falso), ignorar todas las cuarentenas (inseguro).
Elegido: excepción acotada a la identidad de esos dos archivos y solo a este
modo de desarrollo, comprobada bajo lease en cada adquisición. Una nueva
operación usa un journal distinto; su propia incertidumbre bloquea intentos
posteriores. No se inicia inferencia al abrir la app ni se permite concurrencia
con otro propietario. Los hechos históricos permanecen en disco sin cambios.

La inspección posterior no demuestra un cierre retrospectivo ni ausencia de
procesos huidos de un grupo. En cada adquisición se revalidan archivos, PID,
grupo y puerto; inventario y presión se verifican al iniciar el host y siguen
vigentes las guardas ordinarias de recursos por operación. Esto no excluye una
carrera externa en Ollama compartido. Rollback: apagar modo dev y volver al
criterio normal; no
eliminar marcadores, credenciales, sesiones o pesos. Pruebas con fixtures no
demuestran que Gemma funcione en este Mac.
