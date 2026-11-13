# ADR 0017 · tokenizadores grandes sin relajar toda la metadata

2026-10-23 · continuación offline de S3-06. R1 contiene tokenizer.json de11422778
bytes; el cap genérico2 MiB impedía verificarlo antes de cargar. Eso no demuestra
incompatibilidad de inferencia ni autoriza modificar el gate de memoria.

## Decisión y límites

- Solo los nombres planos exactos `tokenizer.json` y `tokenizer.model` pasan a
  cap32 MiB. Config, tokenizer_config, special_tokens, índices, templates y demás
  metadata conservan2 MiB. Presupuesto agregado de metadata48 MiB; assets256 y
  total64 GiB sin cambio. Validar cotas antes del hashing/lectura de archivos.
- La misma función de límites gobierna provider puro y lector nativo; lectura
  nativa hasta cap+1, nunca truncar para aparentar éxito. Symlinks y ownership/
  identidad de archivos mantienen los controles existentes.
- tokenizer.json se valida como un objeto JSON completo sin materializar su
  vocabulario como Value: IgnoredAny más fin de documento explícito. El hash y
  tamaño se conservan; esta comprobación no valida semántica de tokenización,
  template o tools. tokenizer.model sigue siendo opaco, no se interpreta como JSON.
- Provider nativo entrega sus bytes una vez, moviéndolos en lugar de clonarlos;
  no se cambia la interfaz pública del proveedor ni se retienen árboles grandes.
  Config/template/índice mantienen sus validaciones semánticas anteriores.

## Alternativas y migración

Subir todos los límites por comodidad se rechaza: permite config/índices grandes
innecesarios y consumo difícil de acotar. Renombrar/fingir tokenizer para evitar
la validación también se rechaza. Un tokenizer mayor32 MiB sigue bloqueado hasta
otra revisión explícita, no se parte/trunca ni se descarga uno diferente.

Cambio interno aditivo, wire/receta/modelos/runtimes activos intactos. Actualiza
las cotas históricas de ADR0012/13 sin borrar evidencia de sus pruebas anteriores.
Tests sintéticos de límites/nombres/agregado/JSON inválido y smoke nativo sobre
archivos nuevos propios, sin MLX ni modelos personales. Inferencia R1 y tools
siguen bloqueadas por presión warning y no pasan por aceptar un archivo mayor.
