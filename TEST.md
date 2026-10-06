# TEST.md — instalezi agentul și vezi clusterul în dashboard

Ghid pas cu pas, ca în producție: ai un cluster care merge deja, te loghezi pe un nod, instalezi agentul, iar clusterul apare în dashboard.

## Structura repo-ului

Un singur repo, cu trei foldere de livrat:

| Folder | Ce conține | Imagine publicată |
|---|---|---|
| **`hermes-server/`** | Hub-ul (Rust: API, gRPC pentru agenți, alerte, baza de date), site-ul HERMES (dashboard + TV, React + TypeScript) în `web/`, `deploy/` | `hermes-server`, la tag `server-X.Y.Z` |
| **`agent-linux/`** | Agentul pentru Linux (Rust): modul `node` (heartbeat + pod-uri prin runtime), colectorul Kubernetes și cel Swarm | `hermes-agent-linux`, la tag `agent-linux-X.Y.Z` (`linux/amd64`) |
| **`agent-windows/`** | Agentul pentru noduri Docker Swarm cu Windows (Rust; compilat și verificat pe structură, **nerulat încă pe un Windows real**) | `hermes-agent-windows`, la tag `agent-windows-X.Y.Z` (`windows/amd64`) |

`pkg/` și `proto/` conțin codul comun. Imaginile se publică automat din pipeline-ul GitHub Actions (vezi README.md) când dai tag-ul potrivit. Kubernetes **nu** are nevoie de agent Windows: agentul Linux vede tot clusterul prin API, inclusiv nodurile Windows.

## 0. Înainte de orice

**a. Hub-ul trebuie să fie accesibil din cluster.** Agentul se conectează *el* la hub (doar conexiuni de ieșire din cluster). Îți trebuie o adresă a hub-ului pe care o
vede clusterul: `http://IP:8765` în rețea locală sau `https://hub.exemplu.ro` (recomandat, printr-un reverse proxy cu TLS). Verifici de pe un nod al clusterului:

```bash
curl http://ADRESA-HUB:8765/api/health        # trebuie să răspundă {"ok":true}
```

**b. Publică imaginile agentului într-un registry** (o singură dată, apoi la fiecare versiune nouă). Pe GitHub asta se întâmplă automat (GHCR, fără secrete de configurat); mai jos e varianta cu registry propriu.

Din pipeline (un registry propriu, nu GHCR): la repo → Settings → Actions → Secrets, adaugi `REGISTRY_USER` (utilizatorul tău) și `REGISTRY_TOKEN`
(Settings → Applications → Generate token, cu `write:package`). Apoi:

```bash
git tag agent-linux-1.0.0 && git push origin agent-linux-1.0.0        # imaginea Linux
git tag agent-windows-1.0.0 && git push origin agent-windows-1.0.0    # doar dacă ai noduri Windows în Swarm
```

Imaginile apar în registry, la Packages: `registry.exemplu.ro/tu/hermes-agent-linux:1.0.0` și
`registry.exemplu.ro/tu/hermes-agent-windows:1.0.0-ltsc2022` (și `-ltsc2019`). `ltsc2022` = Windows Server 2022, `ltsc2019` = Windows Server 2019:
alegi eticheta care corespunde nodurilor tale. (Pe GitHub, pipeline-ul `publish-*.yml` publică automat pe `ghcr.io/tu/...`, fără secrete de configurat.)

Manual, fără pipeline (nu mai e nevoie de credențiale pentru cod, totul e în repo):

```bash
# agentul Linux, de pe orice mașină cu Docker, din rădăcina repo-ului clonat:
docker login registry.exemplu.ro        # utilizator + token cu write:package
docker buildx build -f agent-linux/Dockerfile --platform linux/amd64 \
  -t registry.exemplu.ro/tu/hermes-agent-linux:1.0.0 --push .
```

```powershell
# agentul Windows: nativ, de pe o mașină Windows cu Docker (Windows containers mode) și Rust instalate — nu se mai cross-compilează de pe Linux:
cargo build --release --locked -p hermes-agent-windows
Copy-Item target/release/hermes-agent.exe agent-windows/agent.exe   # target/ e în .dockerignore, calea asta nu
docker login registry.exemplu.ro
docker build -f agent-windows/Dockerfile `
  --build-arg WINDOWS_BASE=mcr.microsoft.com/windows/nanoserver:ltsc2022 --build-arg VERSION=1.0.0 `
  -t registry.exemplu.ro/tu/hermes-agent-windows:1.0.0-ltsc2022 .
docker push registry.exemplu.ro/tu/hermes-agent-windows:1.0.0-ltsc2022
```

Dacă mașina care construiește nu rezolvă numele registry-ului (numele e doar în `/etc/hosts`), adaugă `--add-host registry.exemplu.ro:IP-UL-REGISTRY`.

**c. Pornește hub-ul, spunându-i ce imagini să pună în manifeste:**

```bash
cd hermes-server/deploy
echo "HUB_SECRET_KEY=$(openssl rand -hex 32)" > .env   # o singură dată; criptează kubeconfig-urile și token-urile de agent în SQLite — păstrează fișierul
AGENT_IMAGE=registry.exemplu.ro/tu/hermes-agent-linux:1.0.0 \
AGENT_IMAGE_WINDOWS=registry.exemplu.ro/tu/hermes-agent-windows:1.0.0-ltsc2022 \
AGENT_PULL_SECRET=registry-cred \
docker compose up -d --build
```

`AGENT_IMAGE_WINDOWS` îl pui doar dacă ai noduri Windows. `AGENT_PULL_SECRET` doar dacă imaginile din registry sunt private (numele secretului îl creezi la pasul 1c).
Dacă pierzi `HUB_SECRET_KEY`, sursele deja adăugate nu mai pot fi decriptate (hub-ul refuză să pornească) — le ștergi și le re-adaugi.

**d. Prima pornire: creezi contul de admin.** Deschide **http://localhost:8765**: la prima vizită apare „Create the admin account" (utilizator, parolă de cel puțin 10 caractere).
În baza de date se păstrează doar hash-ul parolei. Bifa „Allow the wallboard (TV) without login" decide dacă TV-ul (`#/tv`) se poate vedea fără cont: bun pe o rețea de încredere,
de debifat dacă hub-ul e accesibil altora. O schimbi oricând din **Admin → TV display**. Nu expune hub-ul în internet înainte să-ți creezi contul.

## 1. Adaugă și instalează, pe Kubernetes

**a. În dashboard** (după login): Sources → **+ Add source**:

| Câmp | Valoare |
|---|---|
| Name | numele clusterului, de exemplu `prod-eu` |
| Type | `Kubernetes (agent)` |
| Hub address as seen from the cluster | adresa de la pasul 0a (**nu** `localhost`, care e precompletat) |

Apasă **Add**. Apare un dialog cu manifestul YAML (conține token-ul, se arată o singură dată). Apasă **Copy manifest** și lasă dialogul deschis.

**b. Loghează-te pe un nod cu acces de admin** (master-ul sau orice mașină cu `kubectl` și drepturi de admin) și salvează manifestul:

```bash
nano agent.yaml            # lipești manifestul, Ctrl+O, Enter, Ctrl+X
```

**c. Doar pentru imagini private**, creează namespace-ul și secretul de pull înainte să aplici manifestul:

```bash
kubectl create namespace hermes
kubectl -n hermes create secret docker-registry registry-cred \
  --docker-server=registry.exemplu.ro --docker-username=UTILIZATOR --docker-password=TOKEN-CU-read:package
```

Numele `registry-cred` trebuie să fie același cu `AGENT_PULL_SECRET` de la pasul 0c. Nodurile trebuie să poată rezolva `registry.exemplu.ro` și să aibă încredere în certificatul lui.

**d. Aplică manifestul.** (Pe k3s comanda e `sudo k3s kubectl`.)

```bash
kubectl apply -f agent.yaml
```

Se creează namespace-ul `hermes`, un ServiceAccount **doar cu drept de citire**, un ClusterRole și un Deployment cu **un singur pod**, fixat pe **noduri Linux**
(`kubernetes.io/os: linux`), deci nu ajunge niciodată pe un nod Windows. Ajunge un singur agent per cluster, oricâte noduri are: citește prin API-ul clusterului.

**e. Verifică în cluster:**

```bash
kubectl -n hermes get pods                          # 1/1 Running
kubectl -n hermes logs deploy/hermes-agent --tail=5
# așteptat: "collector: connected — 3 nodes · N pods · M volumes"
```

**f. Verifică în dashboard.** În ~10 secunde cardul din Sources devine **Connected** („agent reporting · N nodes").

## 2. Adaugă și instalează, pe Docker Swarm

**a. Adaugă sursa.** Sources → **+ Add source** → Type **`Docker Swarm (agent)`**, același Hub address. Manifestul e un **stack file**. **Copy manifest**.

**b. Loghează-te pe un nod MANAGER** și salvează-l:

```bash
nano agent.yml
```

**c. Registry privat?** Loghează-te în registry pe manager, altfel workerii nu pot trage imaginea:

```bash
docker login registry.exemplu.ro
```

**d. Deploy:**

```bash
docker stack deploy --with-registry-auth -c agent.yml hermes
```

**e. Verifică:**

```bash
docker service ls                             # hermes_agent (și hermes_agent-windows dacă ai pus imaginea Windows)
docker service ps hermes_agent              # un task pe FIECARE nod Linux, toate Running
docker service logs hermes_agent --tail 5
```

Serviciul e **global**: câte un agent pe nod, pentru că doar un nod își poate măsura propriul consum. Fiecare raportează CPU/memoria nodului și a containerelor lui;
cel de pe manager raportează în plus topologia (noduri, servicii, task-uri).

**f.** În ~10 secunde apare **Connected** în Sources, iar clusterul Swarm e pe hartă.

## 3. Clustere hibride (Linux + Windows)

Un cluster poate avea noduri Linux și Windows. Fiecare nod își arată sistemul de operare (eticheta **LINUX** / **WINDOWS** pe host, plus arhitectura în detalii).

| | Kubernetes | Docker Swarm |
|---|---|---|
| Agent pe Linux | un pod, pe un nod Linux | serviciul `agent`, câte unul pe nod Linux |
| Agent pe Windows | **nu e nevoie**: nodurile Windows se văd prin API, cu CPU/memorie din metrics-server | serviciul `agent-windows`, câte unul pe nod Windows (dacă ai pus `AGENT_IMAGE_WINDOWS`) |
| Imaginea Windows | — | `hermes-agent-windows:X.Y.Z-ltsc2022` sau `-ltsc2019`, după versiunea de Windows a nodurilor |

Agentul Windows a fost compilat și imaginea a fost construită și verificată structural, dar **nu a rulat încă pe un nod Windows real**: la prima instalare pe Windows verifică
`docker service ps hermes_agent-windows` și jurnalul lui, și spune-mi ce apare.

## 4. Ce vezi și unde

| Unde | Ce apare |
|---|---|
| **Admin → Sources** | Cardurile surselor: *Connected*, *Connecting*, *Error* (agent tăcut sau cluster inaccesibil), *Duplicate* (același cluster adăugat de două ori) |
| **Admin → Dashboard** | Lista de monitoare (cluster → host → pod/task/volum) cu bare de heartbeat și uptime %. Click: detalii, CPU/memorie, containere, incidente |
| **Admin → Topology** | Harta. Click pe un nod îi arată detaliile; „? Legend" explică simbolurile |
| **`#/tv`** | Ecranul de wallboard: hărți, hosturi, incidente. Nu se mișcă nimic singur |
| **Admin → Alerts** | Istoricul incidentelor, cu „Ack" |
| **Admin → Account** | Schimbi parola. Celelalte sesiuni sunt deconectate |

Nu te speria de nodurile pe care nu le-ai creat tu: pe k3s apar și pod-urile de sistem din `kube-system` (`coredns`, `traefik`, `metrics-server`,
`svclb-traefik-*`, `local-path-provisioner`), instalate de k3s. Singurul lucru adăugat de agent e `hermes-agent`, în namespace-ul `hermes`.

**Uptime history:** bara arată ce a *văzut* hub-ul, deci începe din momentul în care nodul apare prima oară. Intervalul (1h / 6h / 24h / 7d) se alege automat
cât să acopere tot istoricul; îl poți schimba din butoanele de deasupra barei. Dacă ștergi o sursă și o adaugi din nou, nodurile primesc ID-uri noi și
istoricul pornește de la zero.

## 5. Verifică reacția la probleme

Fă asta doar pe un cluster de test.

| Vrei să vezi | Ce faci | Ce apare |
|---|---|---|
| Un pod care crapă | `kubectl run broken --image=busybox -- sh -c "exit 1"` | În câteva secunde: alertă **critică** (`broken CrashLoopBackOff`), romb roșu pe hartă. Îl scoți cu `kubectl delete pod broken` |
| Un serviciu Swarm care crapă | `docker service create --detach --name flaky busybox sh -c "exit 1"` | Alertă `flaky.1 CrashLoop…`. Îl scoți cu `docker service rm flaky` |
| Alerta se închide | repari problema | Se închide după ~15–20 s (pauza împiedică alertele care clipesc) |
| Un nod căzut | oprești un nod | Hostul devine *Unreachable*, pod-urile lui „No data", alertă critică |
| Un agent care tace | `kubectl -n hermes scale deploy hermes-agent --replicas=0` | După ~20 s sursa devine *Error* („agent silent") + alertă. Înapoi cu `--replicas=1` |
| Un cluster adăugat de două ori | adaugi încă o sursă pentru același cluster | A doua primește *Duplicate* și nu desenează nimic |

## 6. Depanare

| Simptom | Cauză | Rezolvare |
|---|---|---|
| Nu îmi amintesc parola de admin | — | Nu există resetare prin interfață. Cu hub-ul oprit ștergi utilizatorul din baza de date: `docker compose exec` nu merge (imaginea nu are shell); folosește `docker compose down -v` (șterge TOT: surse, alerte, istoric) și refaci contul |
| `429 too many attempts` la login | 5 parole greșite în 10 minute de la aceeași adresă | Așteaptă 10 minute |
| Sursa rămâne *Waiting for the agent* | Hub address greșit (ai lăsat `localhost`) sau manifest neaplicat | Șterge sursa și adaug-o din nou cu adresa corectă |
| Agentul scrie `connection refused` / `timeout` către hub | Hub-ul nu e accesibil din cluster (firewall, rețea) | `curl http://ADRESA-HUB:8765/api/health` de pe un nod; deschide portul sau folosește un tunel/reverse proxy |
| Pod-ul agentului: `ImagePullBackOff` / `ErrImagePull` | Imaginea nu e în registry, registry-ul e privat sau nodul nu rezolvă numele | Verifică `AGENT_IMAGE`; pentru imagini private secretul de pull (pasul 1c) și `AGENT_PULL_SECRET`; verifică că nodul rezolvă numele registry-ului |
| În logul agentului: `hub answered 401` | Sursa a fost ștearsă sau tokenul s-a schimbat | Adaugă sursa din nou și aplică manifestul NOU |
| Pipeline-ul: `unauthorized` la push | Lipsesc sau sunt greșite secretele `REGISTRY_USER` / `REGISTRY_TOKEN` | Refă tokenul cu `write:package` și pune-l în secretele repo-ului |
| Swarm: `service mode change is not allowed` | Exista un stack `hermes` dintr-o versiune veche (replicat) | `docker stack rm hermes`, așteaptă câteva secunde, redeploy |
| Swarm: `network hermes_default not found` | Redeploy prea repede după `stack rm` | Așteaptă 10 s și rulează din nou |
| Swarm: hosturile arată „NO METRICS" | Agentul nu rulează pe fiecare nod | `docker service ps hermes_agent` trebuie să arate câte un task pe nod |

## 7. Curățenie

```bash
# în dashboard: Sources → Remove (de două ori, ca confirmare)
kubectl delete namespace hermes && kubectl delete clusterrole,clusterrolebinding hermes-agent     # Kubernetes
docker stack rm hermes                                                                              # Swarm
cd hermes-server/deploy && docker compose down -v      # resetare completă a hub-ului: cont, surse, alerte, istoric, setări
```
