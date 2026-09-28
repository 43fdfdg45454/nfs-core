# CLAUDE.md — nfs-core

Lineamientos vigentes.

## Proyecto

NFSv4.2 para enlaces con latencia y pérdida (una VPN sobre datos móviles), con prioridad en el
streaming de audio y video y en las subidas. Dos repositorios:

- **nfs-core** (este): núcleo en Rust y gateway. Licencia MIT o Apache-2.0.
- **nfs-android** (`43fdfdg45454/nfs-android`): la app, en Kotlin, con su puente al núcleo
  (UniFFI, compilado con cargo-ndk) que depende de nfs-core por git a un commit fijo.
- **Nada propio de Android va en nfs-core** (ni bindings, ni KeyChain, ni el emulador): el núcleo
  ofrece APIs genéricas (por ejemplo, firma TLS delegada) y la app las implementa. El gateway sí
  vive acá.

## Arquitectura

- En el cable, todo es estándar: NFSv4.2 (RFC 7862) sobre ONC RPC (RFC 5531), con el TLS que pida
  el export (RPC-with-TLS, RFC 9289, de punta a punta con tlshd), dentro de una conexión TCP.
- **Transporte QUIC**: cada conexión TCP del cliente viaja en un stream HTTP/3 `CONNECT`
  (RFC 9114) hacia el gateway. Una sola conexión QUIC por servidor, con BBR en los dos sentidos.
  Sin migración: si cambia la IP del cliente, conexión nueva.
- **Transporte TCP**: NFSv4.2 directo por TCP, para servidores sin gateway (NAS cerrados).
- Cada servidor tiene su transporte fijo; no hay detección automática.
- **Gateway** (Rust, contenedor con la red del host y `CAP_NET_ADMIN`): solo traduce el
  transporte, stream ↔ conexión TCP hacia nfsd.
  - Conecta a nfsd **desde la IP real de los paquetes QUIC** (`IP_TRANSPARENT`, marca de socket y
    ruta de política para la vuelta). Nunca inventa ni reemplaza un origen e ignora toda cabecera.
  - No ve el RPC ni el TLS del export, no tiene certificados de cliente propios ni lógica de
    permisos: nfsd y `/etc/exports` deciden exactamente igual que sin gateway.
  - `CONNECT` solo hacia el nfsd configurado. El QUIC exterior pide un certificado de cliente de
    la CA configurada: exigido por defecto, se puede apagar.
- Cuando nfsd hable RPC sobre QUIC (borrador IETF), se agrega como transporte directo; el resto
  del núcleo no cambia.
- El núcleo: XDR/RPC, RPC-with-TLS (rustls, con firma delegada para claves que no salen de
  un almacén externo), cliente NFSv4.2 async (una sesión, varias conexiones unidas con
  `BIND_CONN_TO_SESSION`, canal de callback), pipeline de bloques y caché en disco.

## Plan por etapas

Cada etapa termina con pruebas en la CI y un criterio para pasar a la siguiente.

0. **Base** (hecha): workspace, CI con nfsd, tlshd, exports none/tls/mtls y los perfiles de red;
   iperf3 y cliente del kernel como línea base.
1. **Validar el transporte (criterio de corte)** (hecha, cumple): túnel de prueba del lado del
   cliente (TCP local → `CONNECT` sobre QUIC) y primera versión del gateway. Por el túnel corre
   `nfs-bench` (el cliente del núcleo). Mide 1, 2, 4 y 8 streams contra 32 conexiones TCP,
   lectura y escritura, latencia de un READ urgente con lectura adelantada, nfsd con 8 y 64 hilos,
   CPU por MB y BBR contra CUBIC. Verifica el origen real (exports distintos por IP), que las
   cabeceras no cambian nada, túneles nuevos al cambiar la IP y mTLS de punta a punta. **Pasa si**
   iguala o supera a 32 TCP en lectura y escritura por VPN con 8 hilos de nfsd, y el READ urgente
   no espera detrás de la lectura adelantada. Si no, se para y se revisa el plan.
2. **Protocolo** (hecha; `crates/xdr`, `crates/rpc`, `crates/nfs`): XDR, RPC, RPC-with-TLS y cliente NFSv4.2 por los dos transportes,
   canal de callbacks, delegaciones de lectura, bloqueos y reclamo tras un reinicio. Pruebas de
   resiliencia (corte de red, cambio de IP, respuestas perdidas de RENAME y REMOVE, reinicio de
   nfsd, lease vencido), rechazos de TLS, fechas y permisos de archivos nuevos, bloqueos entre
   clientes y con el cliente del kernel, delegaciones revocadas.
3. **Motor de archivos** (en curso; `crates/engine`, escenarios en `crates/engine/tests`, CI
   `stage3` × variante QUIC × escenario, límites exigidos; TCP solo en las pruebas funcionales): pipeline de bloques, prioridad entre archivos, caché en disco sin huecos.
   Escenarios de carga (películas, escenas, varios reproductores, saltos durante una subida,
   cambios desde otro cliente) con los límites de buena experiencia. **Retomar la comparación de
   la etapa 1 con el cliente propio** (streams, tope de BBR, TCP ×N) en esos escenarios para
   decidir los valores por defecto; revisar también el pico aislado de 1,6–2 s con 8 streams y
   el costo de abrir el primer stream (~400 ms, 4 RTT) frente a "abrir ≤ 1,5 s".
4. **Gateway listo para instalar**: imagen multiarquitectura publicada por la CI, guía y ejemplo de
   compose (en paralelo con la 2).
5–7. App, DocumentsProvider y uso diario: en nfs-android.

## Código (crates)

- `xdr` (nfs-xdr): XDR sin copias. `rpc` (nfs-rpc): registros, llamadas multiplexadas por xid,
  watchdog de inactividad, AUTH_SYS, STARTTLS. `nfs` (nfs-client): estados, atributos, COMPOUND,
  operaciones por tema (`ops/`), sesión (`session/`: slots, reintentos, recuperación, lease),
  API (`client/`). `tunnel` (nfs-tunnel): QUIC, TLS, BBR con tope, cliente del túnel.
  `engine` (nfs-engine): lectura por partes con prioridad, lectura adelantada, caché en disco y
  escritura en paralelo. `testkit`: configuración de las pruebas y red rota a propósito.
  `gateway` y `tools`: binarios.
- Pruebas de integración en `crates/nfs/tests` contra un servidor real por variables de entorno
  (`tests/common`); sin `NFS_SERVER` no hacen nada. Local: `NFS_SERVER=127.0.0.1:2049` con
  nfs-ganesha. Las que rompen la red solo con `NFS_CAN_BREAK` (namespace del cliente en la CI).
- Versión única del workspace (`version` en `Cargo.toml`): un push a `master` que la cambia, con
  la CI en verde, publica el release `v<versión>` y etiqueta la imagen del gateway con ella
  (`ci/release.sh`).
- Decisiones tomadas sin consulta: `DECISIONES.md`.

## Forma de trabajo

- **Nada se ejecuta en la computadora del usuario.** Todo corre en el contenedor de la nube o en
  GitHub Actions.
- Siempre sobre la rama `master`.
- Respuestas en español; explicaciones completas cuando se pide detalle, sin relleno. Código,
  comentarios y mensajes de commit en inglés.
- Las sugerencias del usuario se evalúan antes de seguirlas: si no parecen el camino correcto, se
  dice por qué.
- Cada cambio de transporte o de rendimiento se analiza antes (modelo del enlace, código de punta a
  punta) y se justifica con el impacto esperado; nada "por probar". Se mide antes de construir
  encima.
- Salida mínima: generar solo lo necesario, sin texto de más.
- Antes de cada CI: qué cambió, qué se espera y cuánto tardará aproximadamente.
- Antes de un análisis o de una tanda de cambios: qué se investiga y qué se busca encontrar; al
  terminar, qué se encontró.
- Commits con las líneas de atribución que indique la sesión.
- `README.md` útil y al día para quien llega al repositorio: qué es y por qué (con las mediciones),
  cómo instalarlo, configurarlo y usarlo, qué hacer cuando algo falla, qué garantiza (pruebas,
  seguridad) y cómo se desarrolla. Se actualiza en el mismo commit que cambia algo de eso. En
  inglés, como el código; sin datos de la instalación del usuario (ejemplos con `example.net`).

## Privacidad: el repositorio es público

- **Terminantemente prohibido** publicar detalles de la instalación del usuario: IPs, nombres de
  host o dominios, rutas del servidor o de los exports, usuarios y UID/GID reales, modelo o sistema
  del teléfono, proveedor o ancho de banda contratado, topología de la red o de la VPN,
  certificados, claves, contraseñas o tokens. Vale para código, pruebas, documentación, mensajes
  de commit, issues, PRs y anotaciones o logs de la CI.
- Los ejemplos usan solo direcciones y nombres reservados para documentación: `192.0.2.0/24`,
  `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32` y `example.net` (RFC 5737, 3849, 2606).
  Las redes simuladas de la CI usan direcciones propias de la prueba.
- Certificados y claves de prueba: se generan en la CI en cada corrida y se descartan. Las claves
  reales van solo como secrets de GitHub.
- Lo que el usuario cuente de su servidor, su red o sus logs se usa en la conversación y nunca
  termina en un archivo, un commit ni un mensaje. Estos lineamientos hablan de "el servidor" y de
  "un teléfono" en general.
- Antes de cada commit, revisar el diff buscando esos datos. Si algo se filtró, se corrige y se
  reescribe el historial (force push) en el momento.

## Buena experiencia de uso

Los límites son de buena experiencia, no de "funciona". Toda prueba de rendimiento los exige; donde
los fija la plataforma y no el código (la CPU del emulador), se informan sin exigirlos.

- Abrir un archivo (hasta el primer byte): ≤ 1,5 s.
- Salto dentro de un video: ≤ 1 s en promedio y ≤ 3 s siempre.
- Cerrar un archivo: ≤ 300 ms.
- Volver a algo ya visto: ≤ 100 ms (sale de la caché).
- Reproducción sin cortes: lector a 1 MB/s (1080p) con 2 s de colchón.
- Todo eso por el perfil VPN de referencia: 100 ms de RTT, 0,3 % de pérdida, MTU 1420, 100 Mb/s.

## Estilo de código

- La menor cantidad de código posible.
- Archivos de hasta 100 líneas. Es un límite blando: se pasa solo si dividir el archivo empeora su
  lectura.
- Responsabilidades claras: cada archivo, clase, objeto y modelo hace una cosa.

## Pruebas y CI

- Las pruebas tienen que ser lo más rápidas posible: se reutiliza todo lo que se pueda (entorno,
  servidor, fixtures, compilaciones en caché) y se paraleliza al máximo (matrices, shards, jobs).
- Los minutos de CI no tienen costo en un repositorio público: se usan todos los recursos que
  hagan falta, priorizando siempre el tiempo total por sobre el costo. 20 minutos es el máximo
  aceptable.
- Después de cada push, consultar el estado cada 30 segundos, nunca con esperas largas. Antes de
  ponerse a esperar, explicarle al usuario en un par de palabras qué cambió y qué se espera.
- Los logs de Actions no se pueden leer (403): los resultados se publican como anotaciones
  (`::notice` / `::error`, saltos de línea como `%0A`, sin comas en el título).
- En los scripts: `set -o pipefail` delante de cualquier `| tee`; con `sudo`, pasar `PATH` y
  `GITHUB_ACTIONS` explícitamente.

## Modelo del enlace

- Una conexión TCP con CUBIC y pérdida rinde ≈ MSS/RTT × 1,22/√p: ≈ 0,3 MB/s a 100 ms y 0,3 %.
  Por eso un cliente sobre TCP necesita decenas de conexiones.
- Con BBR la pérdida aleatoria no frena el envío. El producto ancho de banda × RTT a 100 Mb/s y
  100 ms es de ~1,25 MB: pocos READ de 1 MiB en vuelo llenan el enlace.
- Un stream entrega en orden: una pérdida lo frena ~1 RTT. Varios streams separan lo urgente de la
  lectura adelantada.
- Referencia medida (VPN 100 ms, 0,3 %, 100 Mb/s): TCP con 8 flujos (iperf3) 8,5 MB/s.

## CI

- Jobs en paralelo: `privacy` (`ci/privacy.sh`: IPs fuera de loopback y de los rangos de
  documentación, o nombres de redes hogareñas, en archivos o mensajes de commit), `rust` (fmt,
  clippy con `-D warnings`, test) y `link` × perfil.
- Seguridad (workflow `security.yml`, en cada push y a diario): `cargo deny` (`deny.toml`:
  vulnerabilidades y versiones retiradas de RustSec, licencias compatibles, solo crates.io y el h3
  fijado), secretos en todo el historial (`ci/secrets.sh`, gitleaks con su suma verificada),
  zizmor sobre los workflows (sin hallazgos), CodeQL (Rust y Actions, `security-extended`) y
  Trivy sobre la imagen del gateway (críticas o altas con corrección). Los workflows fijan cada
  acción por commit, no guardan la credencial en los checkouts y solo leen salvo el job que
  publica. RPC-with-TLS exige TLS 1.3 (RFC 9289): TLS 1.2 no se compila y el cliente rechaza un
  servidor que lo negocie (`crates/rpc/tests/tls13.rs`).
- Fuzzing (`fuzz/`, cargo-fuzz con nightly, workflow `fuzz.yml`): todo lo que un servidor puede
  mandarle al cliente — registros RPC desde el socket (`records`), los resultados de cada operación
  (`result`), un COMPOUND leído como lo lee el cliente (`compound`) y las llamadas de callback
  (`callback`). Entradas desde los crates con el feature `fuzzing`. 3 min por objetivo en cada
  push, 15 min una vez por día, con el corpus guardado entre corridas; una falla se anota con la
  entrada en base64 para convertirla en prueba.
- `ci/certs.sh` (certificados descartables), `ci/nfsd.sh` (nfsd, tlshd, exports), `ci/link.sh`
  (namespace `cli` detrás de un veth: servidor `198.51.100.1`, cliente `198.51.100.2`; no usar
  `192.0.2.0/24`, que es la red del contenedor de la nube),
  `ci/profiles.sh` (netem por perfil: `vpn` es la referencia de los límites; `lan`),
  `ci/baseline.sh` (iperf3 y cliente del kernel), `ci/run.sh` (errores como anotaciones).
- nfsd aplica la política `xprtsec` de un export anidado solo si es un punto de montaje propio
  (bind mount). tlshd anterior a 0.10 ignora `x509.truststore`: la CA de prueba se instala también
  en el almacén del sistema.
- Etapa 1 (`stage1` × variante, en paralelo): `ci/stage1.sh` compila mientras levanta el
  servidor; `ci/tunnel.sh` arranca el gateway y el cliente
  de túnel (`crates/tools`) con un X-Forwarded-For falso; `ci/transparent.sh` pone las reglas de
  ruteo del origen transparente; `ci/measure.sh` mide lectura, escritura, latencia de un RPC NULL
  bajo carga (`nfs-rpc-probe`) y CPU del túnel por MB; `ci/checks.sh` verifica origen real,
  que un túnel pedido a otro puerto igual llega solo a nfsd, rechazos (CA desconocida, sin certificado), mTLS del export de punta a punta y
  cambio de IP.
- Configurables por servidor (núcleo y app): cantidad de streams (4 por defecto) y tope de datos
  en vuelo de BBR (`bbr/<gain>`, 1,5 BDP por defecto; `crates/tunnel/src/congestion`). El BBR de
  quinn (v1) mantiene ~2 BDP en vuelo y el BDP de más queda en la cola del cuello de botella: con
  el tope, la cola baja de 6–16 MB a 1,3 MB y un RPC chico bajo carga espera ≤ 200 ms.
- Medido en la etapa 1: la ganancia es de BBR, no de QUIC (QUIC con CUBIC: 1,1 MB/s de lectura
  contra 11 con BBR). **BBR es obligatorio** en todo transporte QUIC, incluido uno nativo futuro.
  En cada push: `checks` y `quic-4`; la comparación completa (TCP ×32, 1/2/4/8 streams, sin tope,
  tope 1,25, LAN) corre a mano con `workflow_dispatch`.
- El contenedor de la nube no tiene netem: lo funcional del gateway se puede probar acá con
  namespaces; lo que depende del enlace se mide en la CI.
- Los límites de buena experiencia están en código en `crates/limits` (`nfs-limits`): las pruebas
  de rendimiento los usan con `nfs_limits::check`, nunca con números propios.
