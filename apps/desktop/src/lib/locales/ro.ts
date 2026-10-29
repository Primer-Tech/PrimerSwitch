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
  codexAddApiKey: 'Adaugă o cheie API…',
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
  statusApiKeyDetail: 'Facturat pe token · fără limite de abonament ChatGPT',
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
    'Niciun alt cont nu este pregătit pentru comutarea automată. Verifică starea sa în lista de conturi.',
  orderAfterFirstRead: 'Apare după prima citire.',

  // Next up.
  nextUp: 'Urmează',
  nextWhySoonest: 'Limita sa săptămânală se resetează cel mai curând.',
  nextWhyMostLeft: 'Are cea mai mare rezervă săptămânală.',
  nextAutomatic: 'Comută când {name} ajunge la {threshold}% utilizat.',
  nextWhyPriority:
    'Prioritate {priority} între conturile cu rezervă suficientă.',
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
  fiveHourPrimedLabel: 'Fereastra de 5 ore a pornit',
  scopedStale: 'Datele pe modele sunt vechi.',
  lastReadingKept: 'Ultima citire este păstrată.',
  identityVerified: 'Verificat: autentificarea salvată aparține acestui cont.',
  identityChecking: 'Se verifică autentificarea salvată…',
  identityUnverified: 'Neverificat încă. Actualizează pentru a-l verifica.',
  identitySignIn:
    'Autentificarea salvată nu mai funcționează. Autentifică-te din nou ca să-l folosești.',
  identityApiKey:
    'Folosește o cheie API, facturată pe token, deci nu are limitele unui abonament ChatGPT.',
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
  primeWindow: 'Pornește ferestrele de 5 ore și săptămânală',
  primeHelp:
    'Când fereastra de 5 ore sau cea săptămânală a unui cont se încheie, trimite un mesaj foarte scurt la următoarea verificare pentru a porni fereastra următoare, ca resetarea ei să vină mai devreme. Se aplică și conturilor inactive. Dacă ambele ferestre se încheie simultan, un singur mesaj le pornește pe amândouă.',
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
    'Alege un prag implicit de comutare pentru fiecare furnizor. Din Detalii poți seta un prag propriu, o prioritate și revenirea după resetarea de 5 ore. Comutarea automată, intervalul, ordinea săptămânală, aspectul și limba sunt comune.',
  sharedAutomation: 'Automatizare pentru Claude și Codex',
  claudeOnlyAutomation: 'Doar pentru Claude',
  providerDefaults: 'Praguri implicite pe furnizor',
  providerDefaultsHelp:
    'Procent din cotă utilizat. Fiecare cont moștenește pragul furnizorului dacă nu alegi unul propriu din Detalii.',
  claudeThresholdUsed: 'Prag Claude · % utilizat',
  codexThresholdUsed: 'Prag Codex · % utilizat',
  percentHelp:
    '50–100% utilizat. Sunt acceptate zecimale; derularea nu schimbă valoarea.',
  invalidPercent:
    'Introdu un număr între 50 și 100 cu punct sau virgulă zecimală. Folosește mai puține zecimale dacă este necesar.',
  decreasePercent: 'Scade {label} cu 1 punct procentual',
  increasePercent: 'Crește {label} cu 1 punct procentual',
  providerDefault: 'Pragul furnizorului',
  percentUsed: '{threshold}% utilizat',
  providerDefaultsSaved:
    'Praguri salvate: Claude {claude}% utilizat · Codex {codex}% utilizat.',
  accountAutomation: 'Automatizarea contului',
  accountAutomationHelp:
    'Alege când se comută de pe acest cont și locul său în ordinea automată.',
  includeAutomatic: 'Include în comutarea automată',
  includeAutomaticHelp:
    'Permite selectarea automată a acestui cont. Pe conturile excluse poți comuta în continuare manual.',
  includedAutomatic: 'Inclus în comutarea automată',
  excludedAutomatic: 'Doar manual',
  switchThresholdUsed: 'Comută de pe cont la · % utilizat',
  useProviderDefault: 'Folosește pragul furnizorului ({threshold}% utilizat)',
  customThreshold: 'Prag propriu',
  customThresholdUsed: 'Prag propriu · % utilizat',
  effectiveThresholdUsed: 'Prag de comutare: {threshold}% utilizat.',
  excludedThresholdUsed: 'Doar manual · prag de ieșire: {threshold}% utilizat.',
  rowThresholdUsed: 'Comută la {threshold}% utilizat',
  accountPriority: 'Prioritate (opțional)',
  accountPriorityHelp:
    '1 este cea mai mare; 99 cea mai mică. Lasă gol pentru ordinea săptămânală. Conturile cu rezervă suficientă vin primele; ordinea săptămânală departajează prioritățile egale.',
  invalidPriority: 'Introdu un număr întreg între 1 și 99 sau lasă gol.',
  priorityAutomatic: 'ordine săptămânală',
  priorityBadge: 'Prioritate {priority}',
  returnEnabled: 'revine după resetare',
  statusNeedsRefresh: 'Necesită actualizare',
  claudeLoginAgainTitle: 'Autentifică-te din nou în Claude',
  claudeLoginOriginalWorkspace:
    'Spațiul de lucru inițial: {workspace}. Alege acest spațiu în browser.',
  workspaceIdentifier: 'Spațiu de lucru …{id}',
  claudeLoginAgainDescription:
    'Autentifică-te ca {name} în același spațiu Claude pentru a actualiza acest cont salvat. Un alt cont sau spațiu va fi refuzat; numele, prioritatea și preferințele de automatizare sunt păstrate.',
  claudeLoginAgainNoActivate:
    'Actualizează autentificarea salvată fără a selecta contul ca activ.',
  claudeLoginAgainActive:
    'Actualizează autentificarea salvată și repară sesiunea Claude curentă doar dacă aparține în continuare aceluiași cont și aceluiași spațiu de lucru.',
  loginIdentityMismatch:
    'Autentificarea a returnat alt cont Claude sau alt spațiu de lucru. Alege spațiul de lucru inițial al contului salvat sau folosește Adaugă un cont.',
  saveSignIn: 'Salvează autentificarea',
  updatingSignIn: 'Se actualizează autentificarea salvată…',
  openBrowserAgain: 'Deschide din nou browserul',
  automaticReturns: 'Reveniri automate',
  returnsNotConfigured:
    'Niciun cont nu este setat să revină după resetare. Din Detalii, alege o prioritate și activează revenirea după resetarea de 5 ore.',
  returnsSwitchingOff:
    'Comutarea automată este oprită; revenirile automate sunt suspendate.',
  returnsAccountExcluded:
    '{name}: preferința de revenire este păstrată; revenirile sunt suspendate cât timp contul este exclus.',
  returnsAccountNoPriority:
    '{name}: alege o prioritate din Detalii pentru a activa revenirile.',
  returnsAccountSignIn:
    '{name}: autentifică-te din nou înainte ca revenirile automate să-l poată folosi.',
  returnsAccountUnverified:
    '{name}: identitatea trebuie verificată înainte ca revenirile automate să-l poată folosi.',
  returnsAccountUnconfirmed:
    '{name}: citirea salvată este neconfirmată. Sunt necesare verificări noi înainte de revenire.',
  returnsAccountWeeklyBlocked:
    '{name}: ultima utilizare săptămânală raportată depășește {headroom}% utilizat, necesar pentru revenire (pragul de {threshold}% minus 5 puncte). Resetarea de 5 ore nu reface cota săptămânală.',
  returnsAccountActive: '{name}: contul preferat este deja activ.',
  returnsAccountLowerPriority:
    '{name}: prioritatea {priority} nu este mai mare decât prioritatea activă {activePriority}. Prioritățile egale nu declanșează reveniri.',
  returnsAccountFiveHourBlocked:
    '{name}: ultima utilizare de 5 ore raportată depășește {headroom}% utilizat, necesar pentru revenire. Așteaptă resetarea de 5 ore observată, apoi verificări noi ale rezervei.',
  returnsAccountWatching:
    '{name}: revine când fereastra sa de 5 ore observată recuperează suficientă rezervă. Necesită prioritate strict mai mare și utilizare nou verificată de cel mult {headroom}% în toate ferestrele relevante.',
  returnPriorityRequired:
    'Alege o prioritate pentru revenirile automate sau oprește revenirile.',
  returnExcludedHelp:
    'Preferința de revenire este păstrată. Revenirile sunt suspendate cât timp contul este exclus.',
  returnAfterReset: 'Revino după resetarea de 5 ore',
  returnAfterResetHelp:
    'Necesită includere și o prioritate. După resetarea unei ferestre de 5 ore observate cu rezervă insuficientă, revine dacă prioritatea sa este strict mai mare. Utilizarea nou verificată trebuie să fie cu cel puțin 5 puncte procentuale sub prag în toate ferestrele relevante. Comutarea automată trebuie să fie pornită.',
  manualReturnHelp:
    'Alegerea manuală a unui cont suspendă revenirile automate timp de 15 minute. Le poți relua mai devreme din cardul Automatizare.',
  automationDetailsHelp:
    'Alege pragurile, prioritățile și revenirile automate ale conturilor din Detalii.',
  returnsPaused:
    'Revenirile automate sunt suspendate încă {duration} după alegerea manuală.',
  resumeReturns: 'Reia revenirile automate',
  saveAutomation: 'Salvează automatizarea',
  accountAutomationSaved:
    'Salvat: {threshold}% utilizat ({source}) · {state} · prioritate: {priority} · reveniri: {returnState}.',
  automationUnsupported:
    'Automatizarea este disponibilă pentru conturi ChatGPT salvate cu o configurare acceptată.',
  codexReason_invalidAutomation:
    'Alege un prag între 50 și 100 și, opțional, o prioritate între 1 și 99. Revenirile automate necesită includere și o prioritate.',

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
    'Sesiunea Claude Code necesită reînnoire sau reparare. Poți folosi Claude Code o dată pentru a permite reînnoirea sau poți alege Autentifică-te din nou aici.',
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
    'Cheie API, facturată pe token, fără limitele unui abonament ChatGPT.',
  codexNoWindows: 'Nu au fost raportate limite de utilizare pentru acest cont.',
  codexUsageBlocked: 'Utilizarea inclusă este blocată pentru acest cont.',
  codexSpendBlocked: 'Limita de cheltuieli a spațiului de lucru a fost atinsă.',
  codexCreditsUnlimited: 'Nelimitate',
  codexCreditsAvailable: 'Disponibile',
  codexResetCount: 'Resetări disponibile: {count}',
  codexResetUnknown: 'Disponibilitatea resetărilor este necunoscută',
  codexResetExpiryDates: 'Datele de expirare a resetărilor',
  codexResetOne: '1 resetare',
  codexResetMany: '{count} resetări',
  codexResetExpires: 'Expiră',
  codexResetNoExpiry: 'Nu expiră',
  codexResetExpiryUnknown: 'Datele de expirare sunt indisponibile.',
  codexResetExpiryPartial:
    'Date de expirare disponibile pentru {shown} din {count} resetări.',
  codexUseReset: 'Folosește o resetare',
  codexRetryReset: 'Verifică resetarea anterioară',
  codexUsingReset: 'Se resetează utilizarea…',
  codexRefreshResets: 'Actualizează resetările',
  codexResetHelp:
    'Folosește o resetare disponibilă pe acest cont pentru a redeschide ferestrele de utilizare eligibile.',
  codexResetPending:
    'Resetarea anterioară este neconfirmată. Verific-o din nou pentru a recupera rezultatul fără să folosești încă o resetare.',
  codexResetDone: 'Resetarea a fost aplicată.',
  codexResetCached:
    'Informație salvată. Actualizează pentru a verifica disponibilitatea curentă.',
  codexResetNotLimited:
    'Nu există o fereastră de utilizare eligibilă pentru resetare. Nu s-a folosit nicio resetare.',
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
  codexApiKeyTitle: 'Adaugă o cheie API OpenAI',
  codexApiKeyDescription:
    'Codex facturează cheia API pe token, în afara abonamentului ChatGPT. PrimerSwitch nu comută singur pe ea decât dacă activezi „Folosește ca rezervă” pentru această cheie; altfel o alegi tu cu Comută.',
  codexApiKeyNameLabel: 'Nume',
  codexApiKeyLabel: 'Cheie API',
  codexApiKeyHelp:
    'Începe cu sk-. Se păstrează criptat pe acest computer, iar după salvare se afișează doar ultimele 4 caractere.',
  codexApiKeySave: 'Salvează cheia',
  codexApiKeyEnding: 'Cheie API …{tail}',
  codexApiKeyEndingLabel: 'Cheia API se termină în',
  codexApiKeyAdded:
    'S-a salvat {name} (cheie API …{tail}). Verifică dacă ultimele 4 caractere se potrivesc cu lista de chei de pe platform.openai.com. Codex continuă să folosească contul curent până comuți.',
  codexApiKeyAddedNoTail:
    'S-a salvat {name}. Codex continuă să folosească contul curent până comuți.',
  codexApiKeyExisting:
    'Cheia API era deja salvată, deci nu s-a schimbat nimic.',
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
    'Acest cont folosește o autentificare pe care PrimerSwitch nu o poate comuta.',
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
  codexReason_resetUnavailable:
    'Actualizează pentru a verifica resetările disponibile. Codex poate să nu ofere informația pentru acest cont sau spațiu de lucru.',
  codexReason_noResetCredits:
    'Nu există resetări disponibile pentru acest cont.',
  codexReason_resetUnconfirmed:
    'Rezultatul resetării nu a putut fi confirmat. Verifică resetarea anterioară pentru a recupera rezultatul.',
  codexReason_invalidApiKey:
    'Introdu un nume de cel mult 60 de caractere și o cheie API OpenAI validă, care începe cu sk-. Nu s-a salvat nimic.',
  codexReason_flexUnsupportedVersion:
    'Flex necesită Codex 0.163.0 sau mai nou. Actualizează Codex ca să-l folosești.',
  codexReason_flexConfigFailed:
    'Flex nu a putut fi activat, pentru că fișierul config.toml al Codex nu a putut fi actualizat. Nu s-a schimbat nimic.',
  codexReason_flexRestoreFailed:
    "Flex este încă setat în config.toml al Codex. O sesiune pornită acum poate eșua pe ChatGPT cu 'Unsupported service_tier: flex'. PrimerSwitch încearcă din nou în fiecare minut.",
  codexFlexLabel: 'Folosește procesarea Flex',
  codexFlexHelp:
    'Mai ieftin, dar mai lent, iar o cerere poate eșua când capacitatea Flex este plină; Codex nu reîncearcă această eroare. Doar pentru această cheie API. Sesiunile deja deschise își păstrează nivelul curent.',
  codexFlexTag: 'Flex',
  codexFlexOn:
    'Flex este activ. Sesiunile Codex noi folosesc Flex; sesiunile deja deschise își păstrează nivelul.',
  codexFlexOff:
    "Flex este dezactivat din nou. O sesiune pornită cât timp Flex era activ poate eșua pe ChatGPT cu 'Unsupported service_tier: flex'; pornește o sesiune nouă.",
  codexReason_fallbackNeedsAutoSwitch:
    'Rezerva automată funcționează doar cât timp comutarea automată este pornită. Pornește-o din setările de automatizare Codex.',
  codexFallbackLabel: 'Folosește ca rezervă',
  codexFallbackHelp:
    'Când toate conturile ChatGPT sunt la limită, PrimerSwitch comută singur pe această cheie și revine când un cont se reface. Codex facturează această cheie per token.',
  codexFallbackTag: 'Rezervă',
  codexFallbackConfirmTitle: 'Permiți facturarea automată pe această cheie?',
  codexFallbackConfirmText:
    'Codex facturează această cheie per token, în afara planului tău ChatGPT. PrimerSwitch va comuta singur pe ea când toate conturile ChatGPT sunt la limită și va reveni când unul se reface. Setează un buget lunar pe proiectul OpenAI.',
  codexFallbackConfirmAction: 'Pornește rezerva',
  codexFallbackFlexWarning:
    'O sesiune Codex pornită cât timp această cheie este activă cu Flex eșuează pe ChatGPT după revenirea automată, până pornești una nouă.',
  codexFallbackBanner:
    'Se folosește cheia API {name} (facturată per token) din {time}, pentru că toate conturile ChatGPT sunt la limită. PrimerSwitch revine automat când unul se reface.',
  codexFallbackSwitchBack: 'Revino acum',
};
