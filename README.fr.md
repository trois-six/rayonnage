# Rayonnage

*[English version](README.md)*

Gestionnaire de bibliothèque de BD, comics et mangas en Rust + GTK 4 (Linux, Windows, macOS).
Il reprend une partie de [ComicRack CE](https://github.com/maforget/ComicRackCE), ainsi que les
fonctions des plugins [Bedetheque Scrapper 2](https://github.com/maforget/Bedetheque-Scrapper-2)
et [Library Organizer](https://github.com/Stonepaw/comicrack-library-organizer).

## Fonctionnalités

### Bibliothèque
- Scan d'une liste de dossiers définie dans les paramètres (F5), au démarrage en option.
  Les livres dont le fichier a disparu peuvent être retirés automatiquement.
- Scan optimisé pour les disques réseau (sshfs, SMB, NFS) :
  - les dossiers et les livres sont lus en parallèle (16 lectures simultanées par défaut, réglable
    dans les paramètres) ;
  - seuls les livres nouveaux ou modifiés (taille ou date) sont relus ;
  - un CBZ se lit en un seul petit accès à la fin du fichier (répertoire central et ComicInfo.xml),
    sans parcourir les pages ;
  - pour un PDF, le nombre de pages est cherché au début et à la fin du fichier, et le document
    n'est lu en entier qu'en dernier recours.
- Formats lus, comme ComicRack CE : `.cbz` `.zip`, `.cbr` `.rar` (RAR4 et RAR5), `.cb7` `.7z`,
  `.cbt` `.tar`, `.pdf`, `.djvu`. Le vrai format est détecté par les octets magiques, donc un ZIP
  renommé en `.cbr` est accepté.
- Images de pages reconnues : jpg, jpeg, jif, jiff, gif, png, tif, tiff, bmp, webp, heic, heif,
  avif, jp2, j2k, jxl. Elles sont triées dans l'ordre naturel, et `__MACOSX` est ignoré.
- Lecture de `ComicInfo.xml` dans l'archive, ou d'un fichier XML placé à côté du livre.
- Tableau aux colonnes de ComicRack : série, numéro, volume, titre, parution, auteurs, éditeur,
  collection, genre, étiquettes, personnages, notes, champs catalogue (prix, état, ISBN…), champs
  fichier (nom, chemin, taille, format, dates), etc.
  - Colonnes triables, redimensionnables et déplaçables ; le choix des colonnes se fait via Menu → Colonnes….
  - « Valeurs proposées » : quand la série et le numéro sont vides, ils sont déduits du nom du fichier
    et affichés en grisé, comme dans ComicRack.
  - Édition directe dans le tableau, comme dans ComicRack : un clic lent sur une cellule de la
    ligne sélectionnée (ou F2) l'édite, avec des suggestions tirées de toute la bibliothèque.
    Tab / Maj+Tab passent à la colonne éditable suivante / précédente, ↑ / ↓ à la ligne
    suivante / précédente, Entrée valide, Échap annule. La valeur s'applique à tous les livres
    sélectionnés ; modifier le nom de fichier renomme le fichier.
  - Les colonnes numériques (numéro, volume, nombre, année, mois, jour…) des lignes sélectionnées
    ont des boutons ▲▼ pour incrémenter ou décrémenter la valeur.
  - Recherche rapide (Ctrl+F), filtre par dossier et par « infos non enregistrées » dans la barre
    latérale.
  - Panneau d'information avec la couverture.
- Interface en français et en anglais : elle suit la langue du système, et peut être forcée dans
  Paramètres → Bibliothèque (pris en compte au prochain démarrage).
- Suivi de l'activité :
  - Pendant un scan, une barre de progression et un indicateur en bas à droite s'affichent, et les
    livres apparaissent au fur et à mesure.
  - Le **Journal des messages** (Ctrl+J, ou le bouton « Journal » en bas à droite) détaille chaque
    opération : fichiers ajoutés, convertis ou écrits, et erreurs. Il est aussi copié dans le fichier
    `journal.log` du dossier de cache.

### Édition des informations (Ctrl+I)
Boîte « Informations » sur le modèle de ComicRack, pour un ou plusieurs livres.
- Onglets :
  - **Résumé** : couverture, notes, web.
  - **Détails** : série, numéros, arc, dates, éditeur, auteurs, langue, manga, N&B…
  - **Intrigue et notes** : résumé, notes, critique, personnages, équipes, lieux.
  - **Catalogue** : ISBN, prix, état, propriétaire…
  - **Personnalisé** : valeurs libres.
  - **Pages** : type de chaque page et signets.
- En multi-sélection, seuls les champs modifiés sont appliqués.
- Des suggestions sont tirées des valeurs déjà présentes dans la bibliothèque.
- Boutons Précédent/Suivant.

### Mise à jour des fichiers
- **Mettre à jour les fichiers** (Ctrl+S) écrit `ComicInfo.xml` dans les CBZ/ZIP, CB7/7Z et CBT/TAR,
  et dans les CBR/RAR si le programme `rar` est installé. Les livres modifiés mais pas encore écrits
  sont marqués ✎. Une option permet l'écriture automatique après chaque modification.
  - Dans un CBZ, le nouveau ComicInfo.xml est ajouté à la fin du fichier sans réécrire les pages :
    quelques Ko écrits au lieu de tout le livre. Le reste du fichier n'est pas touché.
  - Comme dans ComicRack, les pages ne sont ni renommées ni modifiées.
- **Rafraîchir depuis les fichiers** relit les informations. La version « forcer » remplace les
  valeurs de la bibliothèque.

### Numérotation automatique (Ctrl+Maj+N)
Reprend l'« Autonumber Wizard » : il numérote la série ou la série alternative dans l'ordre de la
liste, à partir d'un numéro donné, et peut enregistrer le nombre total.

### Conversion / export
- Préréglages **Convertir en CBZ / CB7 / CBR**, qui remplacent le livre dans la bibliothèque.
- Boîte **Exporter…** complète :
  - **Destination** : dossier choisi, même dossier, ou remplacement.
  - **Options de fichier** : suppression de l'original, ajout à la bibliothèque, écrasement,
    combinaison de plusieurs livres en un.
  - **Nommage** : nom du fichier, libellé du livre, ou nom personnalisé numéroté.
  - **Format de sortie** : CBZ, CBR, CB7, CBT, PDF ou dossier d'images ; niveau de compression ;
    inclusion de ComicInfo.xml.
  - **Pages** : retrait des pages par type, sélection de pages (`1-5,8`), étiquettes ajoutées.
  - **Noms des pages** : comme dans ComicRack, elles sont renommées `P00001.jpg`, `P00002.jpg`…
    (suivis de « - signet » si la page en a un) et placées à la racine de l'archive. L'option
    « Conserver les noms d'images d'origine » garde les noms sans les sous-dossiers ; un doublon
    reçoit `_2`, `_3`…
  - **Images** : conversion en JPEG, PNG, WebP, GIF, TIFF ou BMP avec réglage de la qualité ;
    redimensionnement ; pages doubles (couper, pivoter, adapter) ; luminosité, contraste,
    saturation, gamma, netteté, contraste automatique.
- Les préréglages personnels sont enregistrables.

### Bedetheque Scraper 2 (Ctrl+B)
Portage du plugin. La recherche de la série, de l'album et de l'édition se fait sur bedetheque.com,
avec une fenêtre de choix en cas d'ambiguïté.
- Champs récupérés : série, titre, numéro, auteurs, éditeur, collection, dépôt légal, ISBN, format,
  genre, résumé, note, nombre d'albums, série terminée, langue, lien web.
- Chaque champ peut être activé ou désactivé, avec les options du plugin : articles, majuscules,
  « One Shot », délai entre les livres, etc.
- Commandes supplémentaires : « QuickScrape » à partir d'une URL, et re-scrape à partir du champ Web.
- Si bedetheque.com refuse les requêtes directes (erreur 403, délai dépassé), l'application
  réessaie avec `curl`, présent par défaut sur Linux, macOS et Windows 10+.

### Library Organizer (Ctrl+L)
Portage du plugin : il déplace, copie ou simule le rangement des fichiers selon des modèles de
dossier et de nom de fichier.
- Exemple de modèle : `{<publisher>}/{<series>}{ (<startyear>)}` et `{<series>}{ #<number3>}`.
- Profils multiples, prévisualisation en direct.
- Options de rangement : valeurs vides de remplacement, caractères interdits, noms des mois,
  exclusions par dossier ou par règles, suppression des dossiers vides, gestion des doublons
  (remplacer, ignorer, renommer).
- Rapport détaillé et annulation du dernier rangement.

## Compilation

Il faut Rust (édition 2024, donc rustc ≥ 1.85), GTK ≥ 4.12 et un compilateur C++ pour la
bibliothèque unrar embarquée.

- **Linux (Debian/Ubuntu)** : `sudo apt install libgtk-4-dev build-essential`
- **Fedora** : `sudo dnf install gtk4-devel gcc-c++`
- **macOS** : `brew install gtk4 pkgconf`
- **Windows** : avec [MSYS2](https://www.msys2.org/) (environnement UCRT64), lancer
  `pacman -S mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-gcc`

Ensuite :

```sh
cargo build --release
./target/release/rayonnage
```

Le workflow `.github/workflows/ci.yml` compile et teste l'application sur les trois systèmes.

### Programmes externes (optionnels)
Leurs chemins se règlent dans Paramètres → Fichiers.
- `rar` (WinRAR / RAR) sert à créer des CBR et à écrire ComicInfo.xml dans un CBR. Le format RAR
  est propriétaire : la lecture est intégrée, mais la création passe par ce programme. ComicRack CE,
  lui, ne crée pas de CBR.
- `pdftoppm` (poppler) sert pour les pages PDF qui ne contiennent pas une simple image.
- `ddjvu` et `djvused` (DjVuLibre) servent à lire les DjVu.

## Données
Les paramètres sont dans `settings.json` et la bibliothèque dans `library.json`, dans les dossiers
de configuration et de données de l'utilisateur :
- Linux : `~/.config/rayonnage` et `~/.local/share/rayonnage`
- Windows : `%APPDATA%\trois-six\rayonnage`
- macOS : `~/Library/Application Support/org.trois-six.rayonnage`

Les vignettes de couverture sont mises en cache.
