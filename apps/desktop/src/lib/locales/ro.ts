import type { en } from './en';
export const ro: Record<keyof typeof en, string> = {
  // Shell, shared by both providers.
  navigation: 'Navigare',
  accounts: 'Conturi',
  settings: 'Setări',
  openSettings: 'Deschide setările',
  localAccounts: 'Gestionare locală a conturilor',
  providers: 'Furnizori de conturi',
  claudeSubtitle: 'Urmărește utilizarea și comută între conturile Claude Code.',
  codexSubtitle:
    'Urmărește utilizarea și comută între conturile Codex — terminalele deschise se reconectează singure.',
  updatedAge: 'Actualizat {age}',
  noReadings: 'Nicio citire încă',
  refreshAll: 'Actualizează toate',
  refreshing: 'Se actualizează…',
  preview: 'Previzualizare',
  demoNotice: 'Date demonstrative · acțiunile sunt dezactivate.',
  dismissMessage: 'Închide mesajul',
  close: 'Închide',
  cancel: 'Anulează',
  accountInsights: 'Informații despre conturi',

  // Notices.
  switchingAccount: 'Se comută contul…',
  updatingAccounts: 'Se actualizează conturile…',
  allLimited: 'Toate conturile sunt la limită sau aproape de ea.',
  allLimitedUntil:
    'Toate conturile sunt la limită sau aproape de ea. Primul se eliberează {name}, în {duration}.',
  signInNeeded:
    'Autentifică-te din nou ca să poți folosi în continuare {names}.',

  // Active account card.
  activeAccount: 'Cont activ',
  active: 'Activ',
  refresh: 'Actualizează',
  refreshLabel: 'Actualizează utilizarea pentru {name}',
  defaultModel: 'Model implicit',
  fiveHourWindow: 'Fereastra de 5 ore',
  weekly: 'Săptămânal',
  resetIn: 'Se resetează în {duration}',
  resetUnknown: 'Resetare necunoscută',
  resetting: 'Se resetează',
  usingCredits: 'Folosește credite',
  credits: 'Credite',
  viewDetails: 'Detalii',
  cachedAge: 'Citire salvată · {age}',
  automationWaiting: 'Automatizarea așteaptă o citire actuală.',
  alertThreshold: 'A atins pragul de comutare de {threshold}%',
  switchToNow: 'Comută acum pe {name}',
  noActive: 'Niciun cont activ',
  noActiveClaude:
    'Autentifică-te cu Claude sau importă autentificarea pe care o folosește acum Claude Code.',
  noActiveCodex:
    'Autentifică-te cu ChatGPT sau importă autentificarea pe care o folosește acum Codex.',
  loadingAccounts: 'Se încarcă conturile…',
  loadingState: 'Pregătim ultima stare a conturilor tale.',
  switchingUnavailable: 'Comutarea conturilor nu este disponibilă',

  // Saved accounts.
  savedAccounts: 'Conturi salvate',
  addAccount: 'Adaugă cont',
  addActions: 'Adaugă sau importă un cont',
  claudeAddSignIn: 'Autentifică-te cu Claude…',
  claudeImportCurrent: 'Importă autentificarea curentă din Claude Code',
  claudeImportArchive: 'Importă din ClaudeSwitch…',
  codexAddSignIn: 'Autentifică-te cu ChatGPT…',
  codexImportCurrent: 'Importă autentificarea curentă din Codex',
  codexImportSwitcher: 'Importă din Codex Switcher',
  importing: 'Se importă…',
  emptyTitle: 'Niciun cont salvat încă',
  emptyClaude:
    'Autentifică-te cu Claude sau importă autentificarea pe care o folosește acum Claude Code. Apoi comuți între conturi dintr-un singur clic.',
  emptyCodex:
    'Autentifică-te cu ChatGPT sau importă autentificarea pe care o folosește acum Codex. Apoi comuți între conturi dintr-un singur clic.',
  account: 'Cont',
  status: 'Stare',
  actions: 'Acțiuni',
  next: 'Următorul',
  statusReady: 'Disponibil',
  statusLimited: 'Limită atinsă',
  statusNearLimit: 'Aproape de limită',
  statusChecking: 'Se verifică…',
  statusUnread: 'Neverificat încă',
  statusFailed: 'Verificare eșuată',
  statusFailedDetail: 'Actualizează pentru a reîncerca',
  statusApiKey: 'Cheie API',
  statusApiKeyDetail: 'Fără limite de abonament ChatGPT',
  statusUnsupported: 'Autentificare neacceptată',
  statusCheckFailed: 'Ultima verificare a eșuat',
  statusCantSwitch: 'Nu se poate comuta pe acest cont',
  statusFreeAgain:
    'Probabil disponibil din nou · actualizează pentru confirmare',
  statusLimitFreesIn: '{limit} · se eliberează în {duration}',
  statusLimitResetsIn: '{limit} · se resetează în {duration}',
  statusResetsAvailable: 'Resetări disponibile: {count}',
  statusResetCooldown: 'Resetări posibile în {duration}',
  freesIn: 'Se eliberează în {duration}',
  switch: 'Comută',
  switching: 'Se comută…',
  signInAgain: 'Autentifică-te din nou',
  moreLabel: 'Mai multe acțiuni pentru {name}',
  delete: 'Șterge',
  switchBusy: 'Așteaptă finalizarea acțiunii curente.',
  demoOnly: 'Previzualizarea este doar pentru citire.',
  switchChecking: 'Se verifică utilizarea…',
  switchUnverified:
    'Contul nu a putut fi verificat încă. Actualizează-l pentru a reîncerca.',

  // Usage order.
  usageOrder: 'Urmează, în ordine',
  orderEmpty:
    'Niciun alt cont nu mai are utilizare disponibilă sub pragul de comutare.',
  orderAfterFirstRead: 'Apare după prima citire.',

  // Next up.
  nextUp: 'Urmează',
  nextWhySoonest: 'Limita sa săptămânală se resetează cel mai curând.',
  nextWhyMostLeft: 'Are cea mai mare rezervă săptămânală.',
  nextAutomatic: 'Comută automat la {threshold}%.',
  switchNow: 'Comută acum',
  switchNowLabel: 'Comută acum pe {name}',
  noNext: 'Niciun alt cont nu este pregătit de folosire.',

  // Automation.
  automation: 'Automatizare',
  manageAutomation: 'Gestionează automatizarea',
  autoSwitchOn: 'Comutare automată pornită',
  autoSwitchOff: 'Comutare automată oprită',
  resetOrder: 'Ordine',
  resetOrderSoonest: 'Întâi resetarea cea mai apropiată',
  resetOrderMostLeft: 'Întâi cea mai mare rezervă',
  on: 'Pornit',
  off: 'Oprit',
  codexEnvDaemon: 'Terminalele deschise se reconectează automat.',
  codexEnvNoDaemon:
    'Niciun terminal Codex nu este deschis acum; comutarea se aplică de la următoarea sesiune.',
  codexEnvApps:
    'Aplicația Codex și VS Code își păstrează contul până le repornești.',
  codexEnvOtherClients:
    'Aplicația Codex sau VS Code este deschisă și își păstrează contul până la repornire.',

  // Provider cards.
  creditsRenewal: 'Resetări și reînnoire',
  availableResets: 'Resetări disponibile',
  expiresIn: 'Expiră în {duration}',
  resetCooldown: 'Resetările pot fi folosite din nou în {duration}',
  checkingReset: 'Se verifică rezultatul resetării…',
  estimatedRenewal: 'Reînnoire estimată',
  manualRenewalDescription: 'Estimare introdusă manual, nu dată de facturare.',
  codexSetupTitle: 'Configurare Codex',
  codexCheckSetup: 'Verifică configurarea',
  codexSetupChecking: 'Se verifică configurarea Codex…',
  codexSetupReady: 'Codex {version} · pregătit',
  codexSetupReadyUnknown: 'Codex · pregătit',
  codexSetupNotInstalled: 'Codex nu este instalat',
  codexSetupAttention: 'Codex {version} · necesită atenție',
  codexSetupAttentionUnknown: 'Codex · necesită atenție',
  codexHowSaves:
    'La comutare, noua autentificare este salvată, iar Codex repornește în fundal.',
  codexHowResumes: 'Ce rulează deja se termină mai întâi; nu se închide nimic.',

  // Account details.
  detailsSignIn: 'Autentificare',
  detailsWorkspace: 'Spațiu de lucru',
  detailsChecked: 'Ultima verificare',
  subscription: 'Abonament',
  subscriptionInactive: 'Inactiv',
  subscriptionPaused: 'Suspendat',
  subscriptionCanceled: 'Anulat',
  subscriptionTrial: 'Probă',
  unknownStatus: 'Necunoscut',
  unknown: 'Necunoscută',
  unknownCount: 'Necunoscut',
  renewalLabel: 'Zi de reînnoire pentru {name}',
  renewalDay: 'Ziua {day}',
  manualEstimate: 'Estimare introdusă manual · {date}',
  primedLabel: 'Fereastra săptămânală a pornit',
  scopedStale: 'Datele pe modele sunt vechi.',
  lastReadingKept: 'Ultima citire este păstrată.',
  identityVerified: 'Verificat: autentificarea salvată aparține acestui cont.',
  identityChecking: 'Se verifică autentificarea salvată…',
  identityUnverified: 'Neverificat încă. Actualizează pentru a-l verifica.',
  identitySignIn:
    'Autentificarea salvată nu mai funcționează. Autentifică-te din nou ca să-l folosești.',
  identityApiKey: 'Folosește o cheie API, deci nu are limite de abonament.',
  identityUnsupported:
    'Folosește o autentificare pe care PrimerSwitch nu o poate comuta.',
  codexAuthChatGPT: 'Cont ChatGPT',
  codexAuthApiKey: 'Cheie API',
  codexAuthUnsupported: 'Autentificare neacceptată',
  deleteAccount: 'Șterge contul',
  deleteLabel: 'Șterge contul {name}',
  deleteActiveReason: 'Comută pe alt cont înainte să-l ștergi pe acesta.',

  // Delete confirmation.
  deleteTitle: 'Ștergi contul salvat?',
  deleteClaude:
    '{name} va fi eliminat din PrimerSwitch. Autentificarea curentă din Claude Code rămâne disponibilă.',
  deleteCodex:
    '{name} va fi eliminat din PrimerSwitch. Nu te deconectează din ChatGPT și nu schimbă contul pe care Codex îl folosește acum.',
  deleting: 'Se șterge…',

  // Claude sign-in and import.
  claudeLoginTitle: 'Autentificare cu Claude',
  loginDescription:
    'Continuă autentificarea în browser. După autorizare, copiază codul afișat și lipește-l aici.',
  loginStepOne: 'Autentifică-te în browserul deschis.',
  loginStepTwo: 'Copiază întregul cod, inclusiv',
  loginStepThree: 'Confirmă adăugarea contului.',
  addingAccount: 'Se adaugă contul…',
  openingBrowser: 'Se deschide browserul…',
  authorizationCode: 'Cod de autorizare',
  loginNoActivate: 'Contul adăugat nu devine activ automat.',
  importTitle: 'Importă din ClaudeSwitch',
  importAccounts: 'Importă conturile',

  // Settings.
  automaticSwitch: 'Comutare automată',
  autoSwitchHelp: 'Schimbă contul când utilizarea atinge pragul ales.',
  preferSoonestReset:
    'Folosește întâi contul a cărui limită săptămânală se resetează cel mai curând',
  preferSoonestResetHelp:
    'Consumă întâi ce altfel ar expira nefolosit. Dezactivat: se preferă contul cu cea mai mare rezervă săptămânală.',
  primeWindow: 'Pornește fereastra săptămânală',
  primeHelp:
    'Când fereastra săptămânală a unui cont se încheie, trimite un mesaj foarte scurt pentru a o porni imediat pe următoarea, ca resetarea ei să vină mai devreme.',
  autoResets: 'Folosește automat resetările',
  resetsHelp:
    'Când toate conturile sunt la limită, folosește o resetare disponibilă. Dacă resetarea aparține altui cont, PrimerSwitch comută mai întâi pe acel cont, chiar dacă comutarea automată este oprită. O resetare care expiră curând este folosită pe contul ei, fără comutare.',
  pollInterval: 'Interval de verificare',
  minutes: '{count} min',
  pollingHelp:
    'La fiecare verificare, PrimerSwitch citește limitele contului activ: pentru Claude trimite un mesaj foarte scurt, pentru Codex nu trimite nimic. Aproape de limită, verificările sunt mai dese.',
  switchThreshold: 'Prag de comutare',
  appearance: 'Aspect',
  system: 'Sistem',
  light: 'Luminos',
  dark: 'Întunecat',
  language: 'Limbă',
  save: 'Salvează',
  saving: 'Se salvează…',
  invalidSettings:
    'Alege un interval între 2 și 15 minute și un prag între 50% și 100%.',
  globalPreferences: 'Aspect și limbă',
  settingsProviderScope:
    'Comutarea automată, ordinea resetărilor săptămânale, intervalul de verificare și pragul de comutare se aplică pentru Claude și Codex. Pornirea ferestrei săptămânale și folosirea resetărilor se aplică doar pentru Claude. Aspectul și limba se aplică ambilor furnizori.',
  sharedAutomation: 'Automatizare pentru Claude și Codex',
  claudeOnlyAutomation: 'Doar pentru Claude',

  // Formatting.
  noReading: 'Nicio citire',
  secondsAgo: 'acum câteva secunde',
  usageMeter: '{value} la sută utilizat',
  dataUnavailable: 'Date indisponibile',

  // Claude runtime errors and notices: catalog keys that native replies match exactly.
  actionUnavailable: 'Acțiune indisponibilă.',
  failedAction: 'Acțiunea nu a reușit. Datele salvate au fost păstrate.',
  unsafeData: 'Datele primite nu pot fi afișate în siguranță.',
  fullCodeRequired: 'Lipește codul complet, inclusiv partea de după #.',
  previewDevOnly: 'Previzualizarea este disponibilă doar în dezvoltare.',
  storageError:
    'Datele locale nu au putut fi citite sau salvate. Fișierele originale sunt păstrate.',
  vaultUnavailable:
    'Seiful sistemului este blocat, indisponibil sau neacceptat. Conturile salvate nu au fost modificate.',
  unsupportedContext:
    'Acest context Claude folosește o metodă de autentificare sau o politică incompatibilă cu schimbarea conturilor.',
  unsupportedEnvironment:
    'Comutarea conturilor nu este disponibilă cât timp variabila de mediu {name} este setată. Elimin-o, apoi repornește PrimerSwitch.',
  unsupportedSetting:
    'Comutarea conturilor nu este disponibilă cât timp settings.json din Claude Code conține {name}. Elimină intrarea, apoi repornește PrimerSwitch.',
  vaultIntegrity:
    'Datele criptate nu au putut fi autentificate sau cheia lipsește. Fișierele existente sunt păstrate.',
  missingAccount: 'Contul nu mai este disponibil.',
  externalChange:
    'Autentificarea Claude s-a schimbat. Actualizează înainte de a continua.',
  identityError: 'Identitatea contului nu a putut fi verificată.',
  providerError:
    'Claude nu a furnizat o citire proaspătă. Reîncearcă mai târziu.',
  signInRequired:
    'Claude nu mai acceptă autentificarea salvată a acestui cont. Autentifică-te din nou pentru a-l folosi în continuare.',
  activeSessionExpired:
    'Sesiunea Claude Code pentru acest cont a expirat. Folosește Claude Code o dată pentru a o reînnoi sau autentifică-te din nou acolo.',
  settingsError: 'Setările introduse nu sunt valide.',
  readOnlyError: 'Această vizualizare este doar pentru citire.',
  loginExpired: 'Autentificarea a expirat sau a fost anulată.',
  importExpired: 'Importul nu mai este valid. Selectează din nou dosarul.',
  pendingResetError:
    'Cererea anterioară de resetare trebuie confirmată înainte de o nouă încercare.',
  cliVersionError:
    'Versiunea Claude Code nu a putut fi detectată. Instalează o versiune recunoscută.',
  browserError: 'Browserul nu a putut fi deschis. Reîncearcă autentificarea.',
  loginCodeError: 'Codul de autentificare nu este valid.',
  folderError: 'Dosarul nu a putut fi selectat.',
  folderNotLocal: 'Dosarul selectat nu este local.',
  switchInterrupted:
    'O comutare anterioară a fost întreruptă. Autentificarea curentă din Claude Code a rămas neschimbată; comută din nou dacă este nevoie.',
  switchRecoveryPending:
    'O comutare anterioară nu a putut fi finalizată încă. PrimerSwitch o finalizează înainte de următoarea comutare.',
  outgoingUnverified:
    'Autentificarea din Claude Code a acestui cont expirase, așa că nu a putut fi verificată la comutare. A fost păstrată și se verifică la următoarea actualizare.',
  accountError: '{name}: {reason}',
  autoSwitchFailed: 'Comutarea automată pe {name} nu a reușit. {reason}',

  // Codex.
  codexActionFailed:
    'Acțiunea nu a reușit. Conturile salvate au rămas neschimbate.',
  codexBusy: 'Codex finalizează o altă acțiune pe conturi…',
  codexNoQuota:
    'Utilizarea nu a fost verificată încă. Actualizează ca să citești limitele actuale.',
  codexApiQuota:
    'Acest cont folosește o cheie API, deci nu are limitele unui abonament ChatGPT.',
  codexNoWindows: 'Nu au fost raportate limite de utilizare pentru acest cont.',
  codexUsageBlocked: 'Utilizarea inclusă este blocată pentru acest cont.',
  codexSpendBlocked: 'Limita de cheltuieli a spațiului de lucru a fost atinsă.',
  codexCreditsUnlimited: 'Nelimitate',
  codexCreditsAvailable: 'Disponibile',
  codexOtherLimit: 'Altă limită',
  codexPlanFree: 'Gratuit',
  codexWindowShort: 'Fereastră scurtă',
  codexWindowLong: 'Fereastră lungă',
  codexWindowDaily: 'Zilnic',
  codexSwitchingTo: 'Se comută pe {name}',
  codexStageSaving: 'Se salvează noua autentificare…',
  codexStageRestarting:
    'Codex repornește în fundal. Terminalele deschise se reconectează singure; ce rulează deja se termină mai întâi (până la un minut).',
  codexStageVerifying: 'Se verifică noul cont…',
  codexStepSave: 'Salvare',
  codexStepRestart: 'Repornire',
  codexStepVerify: 'Verificare',
  codexSwitchedLive:
    'Codex folosește acum {name}. Terminalele deschise se reconectează automat.',
  codexSwitchedNext:
    'Codex folosește acum {name}. Schimbarea se aplică de la următoarea sesiune Codex pe care o pornești.',
  codexSwitchedOtherClients:
    'Aplicația Codex și VS Code păstrează contul anterior până le repornești.',
  codexSwitcherTitle: 'Codex Switcher rulează.',
  codexSwitcherText:
    'Rescrie aceeași autentificare și închide forțat terminalele Codex când comută. Închide-l ca să eviți conflictele.',
  codexSwitcherImport: 'Importă-i conturile',
  codexWarnings: 'Avertismente de configurare',
  codexLoginComplete:
    'Cont adăugat. Codex folosește în continuare contul curent până când comuți.',
  codexSignedInAgain: 'Te-ai autentificat din nou ca {name}.',
  codexDeleteComplete: 'Contul salvat a fost șters din PrimerSwitch.',
  codexImportedCurrent: 'Autentificarea Codex curentă a fost salvată.',
  codexImportedCurrentExisting:
    'Autentificarea Codex curentă era deja salvată, așa că a fost actualizată.',
  codexImportedSwitcherOne: 'A fost importat {count} cont din Codex Switcher.',
  codexImportedSwitcherFew:
    'Au fost importate {count} conturi din Codex Switcher.',
  codexImportedSwitcherOther:
    'Au fost importate {count} de conturi din Codex Switcher.',
  codexImportedSwitcherNone:
    'Nu există conturi noi în Codex Switcher; toate erau deja salvate.',
  codexLoginTitle: 'Autentificare cu ChatGPT',
  codexLoginAgainTitle: 'Autentifică-te din nou',
  codexLoginDescription:
    'Finalizează autentificarea în browser. Fereastra se actualizează singură când termini, deci nu trebuie să lipești niciun cod.',
  codexLoginAgainDescription:
    'Autentifică-te în browser ca {name}. Fereastra se actualizează singură când termini, deci nu trebuie să lipești niciun cod.',
  codexLoginKeepsCurrent:
    'Adăugarea unui cont nu schimbă contul folosit de Codex.',
  codexLoginOpening: 'Se deschide browserul…',
  codexLoginWaiting: 'Se așteaptă finalizarea în browser…',
  codexLoginVerifying: 'Se verifică contul…',
  codexReason_notInstalled:
    'Codex nu este instalat pe acest computer. Instalează-l, apoi verifică din nou configurarea.',
  codexReason_unsupportedVersion:
    'Această versiune Codex nu este încă acceptată, așa că schimbarea conturilor este dezactivată.',
  codexReason_unsupportedStore:
    'Codex își păstrează autentificarea într-un loc pe care PrimerSwitch nu îl poate folosi încă. Nu s-a schimbat nimic.',
  codexReason_unsupportedAuth:
    'Acest cont folosește o cheie API sau o altă autentificare care nu poate fi comutată.',
  codexReason_policyRestricted:
    'Politica Codex a organizației tale nu permite schimbarea conturilor.',
  codexReason_externalCredentials:
    'Codex este configurat să folosească o autentificare din altă parte, de exemplu o variabilă de mediu, care poate înlocui contul ales aici.',
  codexReason_storeConflict:
    'Codex are autentificări salvate care intră în conflict. Autentifică-te din nou în Codex, apoi verifică configurarea.',
  codexReason_vaultUnavailable:
    'Stocarea securizată de pe acest computer este blocată sau indisponibilă. Conturile salvate au rămas neschimbate.',
  codexReason_identityUnverified:
    'Acest cont nu a fost confirmat încă. Actualizează-l sau autentifică-te din nou.',
  codexReason_identityMismatch:
    'ChatGPT a returnat alt cont decât cel așteptat. Nu s-a schimbat nimic.',
  codexReason_externalChange:
    'Autentificarea Codex a fost schimbată din afara PrimerSwitch. Verifică configurarea, apoi încearcă din nou.',
  codexReason_loginExpired:
    'Autentificarea a durat prea mult și a expirat. Încearcă din nou.',
  codexReason_loginCanceled: 'Autentificarea a fost anulată.',
  codexReason_providerUnavailable:
    'Codex nu a răspuns cum era de așteptat. Încearcă din nou peste câteva momente.',
  codexReason_busy:
    'O altă acțiune pe conturi este încă în curs. Încearcă din nou peste câteva momente.',
  codexReason_daemonRestartFailed:
    'Noua autentificare este salvată, dar Codex nu a putut reporni în fundal. Terminalele deschise păstrează contul anterior până le repornești.',
  codexReason_signInRequired:
    'Autentificarea salvată nu mai este validă. Autentifică-te din nou ca să folosești acest cont.',
  codexReason_switchInProgress:
    'O altă comutare este deja în curs. Așteaptă să se termine.',
  codexReason_switcherUnavailable:
    'Conturile din Codex Switcher nu au fost găsite sau nu au putut fi citite.',
  codexReason_activeAccount:
    'Comută pe alt cont înainte să-l ștergi pe acesta.',
};
