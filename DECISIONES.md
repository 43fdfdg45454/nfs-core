# Decisiones para revisar

Decisiones vigentes tomadas sin consulta. Cada una dice qué se decidió y por qué.

## Cliente NFSv4.2

1. **Un `owner` por cliente, único.** Dos clientes con el mismo owner se pisan: cada uno parece el
   otro reiniciándose y el servidor tira la sesión del otro (lo vieron las pruebas). La app usa
   un cliente por servidor, con owner = id de instalación + servidor.
2. **Un open-owner por archivo abierto.** Cada `File` tiene su propio estado de apertura, así que
   cerrar uno no cierra otro abierto sobre el mismo archivo.
3. **Recuperación:** si el servidor pierde la sesión o el cliente, se repite EXCHANGE_ID y
   CREATE_SESSION (una vez para todos los que esperan). Con un client id nuevo (el servidor se
   reinició), cada archivo abierto pide su OPEN (`CLAIM_PREVIOUS`) y sus bloqueos (`reclaim`)
   antes de `RECLAIM_COMPLETE`. Si el servidor no está en gracia (el lease de este cliente
   venció), se reabren por handle al usarse y los bloqueos quedan perdidos:
   `File::locks_lost()` lo dice.
4. **Slot con resultado desconocido:** si una operación se abandona por tiempo tras perder la
   conexión, su slot no se vuelve a usar en esa sesión (reusar su número de secuencia podría
   devolver la respuesta de otra operación).
5. **Respuesta que el servidor no guardó** (`RETRY_UNCACHED_REP`, en SEQUENCE o en la operación
   siguiente): lo que puede ejecutarse dos veces (lecturas, WRITE) se repite con el número de
   secuencia siguiente; un cambio de estado devuelve `Uncertain`, porque repetirlo podría
   ejecutarlo dos veces.
6. **Crear en exclusiva con `GUARDED4`**, no `EXCLUSIVE4_1` (con este, el verificador queda en
   las fechas del archivo).
7. **Escrituras `UNSTABLE` + COMMIT**; lo no confirmado se reenvía si cambia el verificador (el
   servidor se reinició).
8. **Tamaño de cada READ/WRITE:** el menor entre 1 MiB, lo que concede la sesión y `maxread` /
   `maxwrite` del export.
9. **Tiempo para conectar en dos partes:** la primera respuesta (nombre y TCP, o el apretón de
   QUIC y el CONNECT) en 4 s como máximo: sin nadie del otro lado se sabe enseguida, y aun con
   500 ms de RTT y un paquete perdido llega antes. Lo que sigue (TLS, la sesión) puede ser lento
   pero avanza: queda bajo el tiempo de inactividad (30 s).
10. **Renovación del lease:** un SEQUENCE solo cada tercio del lease, únicamente si no hubo
    llamadas en ese lapso.
11. **Canal de callbacks en cada conexión:** `CREATE_SESSION` con canal de vuelta y
    `BIND_CONN_TO_SESSION` (ambos sentidos) en cada conexión nueva, para que el servidor tenga por
    dónde llamar aunque se caiga la que usaba. Callbacks con AUTH_NONE, 4 slots, respuesta en
    caché por slot para los reenvíos. Si SEQUENCE avisa que el camino está caído
    (`CB_PATH_DOWN`, `CB_PATH_DOWN_SESSION`, `BACKCHANNEL_FAULT`), la conexión que respondió se
    vuelve a enlazar (a lo sumo cada 2 s); `Client::callbacks_down()` cuenta los avisos.
12. **Solo delegaciones de lectura**, pedidas en los OPEN de solo lectura. Una de escritura
    obligaría a responder `CB_GETATTR` y a vaciar escrituras al devolverla; si el servidor da una
    sin pedirla, se devuelve enseguida. El motor reutiliza una versión abierta sin GETATTR
    mientras tenga la misma delegación. Al escribir por el motor se olvida lo guardado de ese
    archivo: nfsd no le revoca la delegación al mismo cliente que escribe.
13. **Espera de bloqueos:** `LOCK` con los tipos "W" (el servidor puede avisar con
    `CB_NOTIFY_LOCK`) y, por si no avisa, reintentos con pausa creciente (hasta 4 s; 15 s si el
    servidor dijo que avisa). Un bloqueo de lectura necesita el archivo abierto para leer.
14. **Cambio de red avisado por la plataforma:** `Client::network_changed()` cierra las conexiones
    (y la conexión QUIC al gateway) para que las llamadas siguientes reconecten enseguida por la
    red nueva. El núcleo no mira la red del sistema: eso es de la app.
15. **Conexiones por defecto:** 4 streams con QUIC y 8 conexiones con TCP (con 32, varias pruebas
    en paralelo pasaban el máximo de conexiones de nfsd, (hilos + 3) × 20, y nfsd cortaba las de
    más).
16. **Listados con `rdattr_error`:** una entrada cuyos atributos no se pueden leer (un export que
    pide otra seguridad) viene con el motivo en vez de hacer fallar todo el listado.
17. **Permisos de lo nuevo con umask:** un archivo o una carpeta nuevos sin modo pedido se crean
    con `0666` o `0777` menos `Config::umask` (`022` por defecto, como el cliente del kernel). Sin
    modo, nfsd los crea `0000` y después solo root puede volver a abrirlos para escribir.

## Motor de archivos

18. **Conexiones reservadas para lo urgente:** metadatos y lecturas que alguien espera van por
    conexiones que la lectura adelantada y las escrituras no usan, para que su respuesta no quede
    detrás de 1 MiB. Con QUIC basta un stream (todos comparten el control de congestión); con TCP,
    una cuarta parte (mínimo 2).
19. **Lo urgente no pasa por la cola:** una parte que un lector espera se pide directo. La cola
    (READ de 1 MiB en vuelo para todos los archivos, lo más cercano primero; en vuelo, el máximo
    entre 8 y las conexiones de la sesión) es solo para la lectura adelantada; al sacar un bloque
    que ya nadie quiere (hubo un salto), se descarta.
20. **Unidad de caché: partes de 128 KiB.** Un bloque adelantado se pide en un READ de 1 MiB y se
    guarda como 8 partes. En disco, un archivo disperso por versión y un índice de partes: sin
    huecos ni miles de archivos chicos. Una versión nueva borra las viejas.
21. **Ventana de lectura adelantada:** nada tras una sola lectura después de un salto (un cuadro
    al buscar una escena), 4 MiB cuando sigue leyendo, 16 MiB desde 1 MiB leído, y "Leer por
    adelantado" completo (256 MiB; 48 en memoria) desde 8 MiB. Con la ventana completa se rellena
    en tandas: recién cuando lo que hay por delante baja a tres cuartos, para que la radio de un
    teléfono descanse entre tandas (a 1 MB/s, un minuto) sin que el colchón baje de 192 MiB.
22. **Escritura:** WRITE del tamaño de la sesión, en vuelo como la lectura adelantada. Cada 64 MiB
    sin confirmar, un COMMIT en segundo plano de lo ya recibido, sin frenar el envío (uno a la vez:
    128 MiB sin confirmar como máximo); al cerrar, COMMIT de todo.
23. **Rutas en una llamada con cada paso a la vista** (`Client::walk`): LOOKUP, GETFH y GETATTR
    por componente, hasta 64 operaciones por COMPOUND (lo que conceda el servidor; si concede
    menos, en tramos). Un enlace simbólico en el camino es el último paso: el LOOKUP siguiente
    falla. Antes, 16 operaciones: una ruta de más de 11 componentes no se abría.
24. **Prioridad de los lectores sobre las subidas:** mientras un lector espera la red, las
    escrituras bajan a 2 en vuelo.
25. **Archivos cambiados por otro cliente, como el cliente NFS del kernel** (consistencia al
    abrir): mientras está abierto, lo ya leído puede quedar viejo; al reabrir, todo es la versión
    nueva. Un archivo que crece se lee más allá del fin viejo por el mismo lector (lo que pasa del
    tamaño conocido se pide directo, sin caché); uno truncado termina en su nuevo fin al instante.
26. **Borrado por otro cliente: la caché del archivo viejo queda** hasta que la desaloje el límite.
    El motor no puede saberlo, y nunca se sirve para un archivo nuevo con el mismo nombre (la
    caché va por handle y versión). Borrado desde la app: se descarta en el momento.

27. **Memoria total del motor: 96 MiB** para todos los archivos abiertos, repartida
    entre ellos (dos tercios adelante, uno atrás; nunca más que los 48 / 24 MiB por archivo). Lo que
    no entra en memoria queda en la caché de disco. `Engine::memory()` informa uso y pico.

## Herramientas y proceso

28. **h3 desde git**, fijado a un commit: la versión publicada (0.0.8) manda `:scheme` y `:path`
    en un CONNECT común, cosa que RFC 9114 prohíbe. Volver a crates.io cuando salga la versión.
29. **rustfmt con `use_small_heuristics = "Max"`**: el mismo código en menos líneas, para el
    límite de 100 por archivo.
30. **Servidor local para desarrollar: nfs-ganesha** en el contenedor (su kernel no tiene nfsd).
    La referencia sigue siendo nfsd en la CI.
31. **Red simulada en `198.51.100.0/24`**: el contenedor de la nube usa `192.0.2.0/24`.
32. **Fixtures que las pruebas alteran** (`gone-*`, `edited-*`, `grown-*`, `shrunk-*`): se
    escriben en el servidor en cada job de la CI; localmente hay que volver a generarlas
    (`ci/fixtures.sh`) antes de cada corrida.
33. **QUIC: un ping cada 25 s y 60 s de inactividad** (antes 2 s y 10 s): con pings cada 2 s la
    radio de un teléfono nunca vuelve a reposo (tarda unos 10 s); 25 s conserva los NAT (30 s como
    mínimo). Un camino muerto con llamadas esperando lo detectan el tiempo de inactividad del RPC
    (30 s); al reconectar, un CONNECT sin respuesta del gateway en 4 s hace que el túnel descarte la
    conexión QUIC muerta y abra otra: como por TCP. Los cambios de red avisados por la plataforma reconectan
    al instante.
