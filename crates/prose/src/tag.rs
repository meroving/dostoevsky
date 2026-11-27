//! A word tagger with no model, for systems that have none (the Mac's NaturalLanguage tags
//! there): word classes from each language's closed-class words and a few endings, base words
//! from Snowball stemmers. Enough for passive marks and echoes, not a grammar: an open-class word
//! is a noun unless its ending says verb or adverb.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

use english::is_word_char;
use rust_stemmers::{Algorithm, Stemmer};

use crate::passive::{EN_IRREGULAR, ES_IRREGULAR};
use crate::token::{Class, Token};
use crate::Lang;
use Class::{Adverb, Conjunction, Determiner, Particle, Preposition, Pronoun, Verb};

/// Words as NaturalLanguage splits them (`can’t` is `ca` + `n’t`, `l’écriture` is `l’` +
/// `écriture`), each with a class and a lowercase base word.
pub fn tag(text: &str, lang: Lang) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let table = table(lang);
    let stemmer = Stemmer::create(algorithm(lang));
    let mut out: Vec<Token> = Vec::new();
    for r in split(&chars, lang) {
        let word: String = chars[r.clone()].iter().collect();
        let w = normal(&word);
        let (class, lemma) = if let Some(&(class, lemma)) = table.get(w.as_str()) {
            (class, lemma.map_or_else(|| w.clone(), str::to_string))
        } else if w.chars().any(char::is_numeric) && !w.chars().any(char::is_alphabetic) {
            (Class::Number, w.clone())
        } else {
            let (class, lemma) = open(&w, &word, lang, &out);
            (class, lemma.unwrap_or_else(|| stemmer.stem(&w).into_owned()))
        };
        let lemma = if lang == Lang::Ru && class == Verb && lemma == w {
            // The stemmer keeps short participles whole (взят); passive marks need them read as forms.
            w.trim_end_matches(['а', 'о', 'ы']).trim_end_matches(['т', 'н']).to_string()
        } else {
            lemma
        };
        out.push(Token { range: r, word, class, lemma: Some(lemma) });
    }
    out
}

fn algorithm(lang: Lang) -> Algorithm {
    match lang {
        Lang::En => Algorithm::English,
        Lang::Ru => Algorithm::Russian,
        Lang::De => Algorithm::German,
        Lang::Fr => Algorithm::French,
        Lang::Es => Algorithm::Spanish,
        Lang::It => Algorithm::Italian,
        Lang::Pt => Algorithm::Portuguese,
    }
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’')
}

/// Lowercase, with a straight apostrophe.
fn normal(word: &str) -> String {
    word.chars().map(|c| if c == '’' { '\'' } else { c }).collect::<String>().to_lowercase()
}

fn split(chars: &[char], lang: Lang) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let mut s = i;
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        let mut e = i;
        while s < e && !chars[s].is_alphanumeric() {
            s += 1;
        }
        while e > s && !chars[e - 1].is_alphanumeric() {
            e -= 1;
        }
        if s < e {
            clitics(chars, s..e, lang, &mut out);
        }
    }
    out
}

const EN_CLITICS: &[&str] = &["s", "re", "ve", "ll", "d", "m"];
const FR_ELISIONS: &[&str] = &["l'", "d'", "j'", "m'", "n'", "s'", "t'", "c'", "qu'", "jusqu'", "lorsqu'", "puisqu'", "quoiqu'", "presqu'"];
const IT_ELISIONS: &[&str] = &["l'", "un'", "d'", "dell'", "all'", "dall'", "nell'", "sull'", "coll'", "c'", "quell'", "quest'", "bell'", "sant'", "m'", "t'", "s'", "v'", "n'"];

fn clitics(chars: &[char], r: Range<usize>, lang: Lang, out: &mut Vec<Range<usize>>) {
    let w = normal(&chars[r.clone()].iter().collect::<String>());
    let n = r.len();
    match lang {
        Lang::En => {
            if n > 3 && w.ends_with("n't") {
                out.push(r.start..r.end - 3);
                out.push(r.end - 3..r.end);
                return;
            }
            if let Some(k) = (r.start + 1..r.end).rev().find(|&k| is_apostrophe(chars[k])) {
                if EN_CLITICS.contains(&&w[w.rfind('\'').unwrap() + 1..]) {
                    out.push(r.start..k);
                    out.push(k..r.end);
                    return;
                }
            }
        }
        Lang::Fr | Lang::It => {
            let elisions = if lang == Lang::Fr { FR_ELISIONS } else { IT_ELISIONS };
            if let Some(k) = (r.start + 1..r.end - 1).find(|&k| is_apostrophe(chars[k])) {
                let head: String = w.chars().take(k - r.start + 1).collect();
                if elisions.contains(&head.as_str()) {
                    out.push(r.start..k + 1);
                    clitics(chars, k + 1..r.end, lang, out);
                    return;
                }
            }
        }
        _ => {}
    }
    out.push(r);
}

type Table = HashMap<&'static str, (Class, Option<&'static str>)>;

fn table(lang: Lang) -> &'static Table {
    static TABLES: [OnceLock<Table>; 7] = [const { OnceLock::new() }; 7];
    TABLES[lang as usize].get_or_init(|| {
        let mut t = Table::new();
        for &(class, lemma, words) in closed(lang) {
            for w in words.split_whitespace() {
                t.entry(w).or_insert((class, lemma));
            }
        }
        t
    })
}

/// Closed-class words, lowercase with a straight apostrophe: (class, base word or the word itself, words).
/// The first list a word is in wins. The commonest adverbs are particles here, so they never echo.
fn closed(lang: Lang) -> &'static [(Class, Option<&'static str>, &'static str)] {
    match lang {
        Lang::En => &[
            (Verb, Some("be"), "be am is are was were been being 'm 're"),
            (Verb, Some("have"), "have has had having 've"),
            (Verb, Some("do"), "do does did doing"),
            (Verb, Some("will"), "will 'll wo"),
            (Verb, Some("would"), "would 'd"),
            (Verb, Some("can"), "can ca cannot"),
            (Verb, Some("shall"), "shall sha"),
            (Verb, None, "should could may might must"),
            (Particle, Some("not"), "not n't"),
            (Pronoun, None, "i me my mine myself you your yours yourself yourselves he him his himself she her hers herself it its itself we us our ours ourselves they them their theirs themselves who whom whose which what whatever whoever someone somebody something anyone anybody anything everyone everybody everything nobody nothing none one ones"),
            (Determiner, None, "the a an this that these those some any no every each all both either neither another such"),
            (Preposition, None, "of in on at by for with from to into onto about above below over under after before between through during without within against among around across behind beyond toward towards upon near off out up down since until like per via"),
            (Conjunction, None, "and or but nor so yet if because although though while when whereas unless than whether as once whenever wherever"),
            (Particle, None, "very just also too then there here now only even still again ever quite rather soon already"),
            (Adverb, None, "never always often sometimes almost perhaps maybe however instead together later away back"),
        ],
        Lang::Ru => &[
            (Verb, Some("быть"), "быть был была было были буду будешь будет будем будете будут будь будьте будучи есть"),
            (Verb, Some("стать"), "стать стал стала стало стали стану станешь станет станем станете станут"),
            (Verb, Some("мочь"), "мочь могу можешь может можем можете могут мог могла могло могли"),
            (Pronoun, None, "я ты он она оно мы вы они меня тебя его её ее него неё нее нас вас их них мне тебе ему ей нему ней нам вам им ним ими ними мной мною тобой тобою нами вами себя себе собой мой моя моё мое мои моего моей моих моим моему твой твоя твоё твое твои твоего твоей свой своя своё свое свои своего своей своих своим наш наша наше наши нашего нашей ваш ваша ваше ваши кто что кого чего кому чему кем чем ком чём который которая которое которые которого которой которых чей чья чьё чьи никто ничто ничего никого нечто некто кто-то что-то"),
            (Determiner, None, "этот эта это эти этого этой этих этим этому тот та то те того той тех тем тому весь вся всё все всего всей всех всем всеми сам сама само сами самого такой такая такое такие такого каждый каждая каждое какой какая какое какие"),
            (Preposition, None, "в во на с со к ко по о об обо от ото из изо у за над надо под подо при про для без до через перед передо между после около ради сквозь среди возле мимо вокруг"),
            (Conjunction, None, "и а но или да что чтобы если когда как потому поэтому хотя пока чем зато либо будто словно ибо"),
            (Particle, None, "не ни же ж бы б ли ль вот уж уже ещё еще даже лишь только разве неужели вон ведь очень тут там здесь где куда откуда тогда теперь сейчас так тоже также"),
            (Adverb, None, "всегда никогда потом опять снова совсем почти вдруг давно долго скоро"),
        ],
        Lang::De => &[
            (Verb, Some("werden"), "werden werde wirst wird werdet wurde wurdest wurden wurdet worden geworden würde würdest würden würdet"),
            (Verb, Some("sein"), "sein bin bist ist sind seid war warst waren wart gewesen sei seien wäre wären"),
            (Verb, Some("haben"), "haben habe hast hat habt hatte hattest hatten gehabt hätte hätten"),
            (Verb, Some("können"), "können kann kannst könnt konnte konnten könnte könnten gekonnt"),
            (Verb, Some("müssen"), "müssen muss musst müsst musste mussten müsste müssten"),
            (Verb, Some("sollen"), "sollen soll sollst sollt sollte sollten"),
            (Verb, Some("wollen"), "wollen will willst wollt wollte wollten"),
            (Pronoun, None, "ich du er sie es wir ihr mich dich sich uns euch mir dir ihm ihn ihnen mein meine meinen meinem meiner meines dein deine deinen deinem deiner seine seinen seinem seiner seines ihre ihren ihrem ihrer ihres unser unsere unseren unserem euer eure man wer was wen wem jemand niemand nichts etwas alles"),
            (Determiner, None, "der die das den dem des ein eine einen einem einer eines kein keine keinen keinem keiner dieser diese dieses diesem diesen jener jene jenes welche welcher welches jede jeder jedes jeden alle"),
            (Preposition, None, "in im ins an am ans auf aus bei beim mit nach von vom zu zum zur über unter vor hinter neben zwischen durch für gegen ohne um bis seit während wegen trotz"),
            (Conjunction, None, "und oder aber denn sondern dass ob wenn als weil obwohl damit doch sowie"),
            (Particle, None, "nicht nur auch schon noch ja so sehr hier dort da dann jetzt nun mal eben"),
            (Adverb, None, "immer nie oft wieder gestern heute morgen bald fast"),
        ],
        Lang::Fr => &[
            (Verb, Some("être"), "être suis es est sommes êtes sont étais était étions étiez étaient été serai seras sera serons serez seront serais serait seraient sois soit soyons soient fut furent"),
            (Verb, Some("avoir"), "avoir ai as a avons avez ont avais avait avions aviez avaient eu aurai aura aurons auront aurais aurait auraient aie ait aient"),
            (Verb, Some("pouvoir"), "pouvoir peux peut pouvons pouvez peuvent pouvait pouvaient pu pourrait"),
            (Verb, Some("devoir"), "devoir dois doit devons devez doivent devait devaient dû devrait"),
            (Pronoun, None, "je j' tu il elle on nous vous ils elles me m' te t' se s' lui leur y en moi toi soi eux qui que qu' quoi dont où lequel laquelle lesquels celui celle ceux celles ceci cela ça c' rien personne quelqu'un"),
            (Determiner, None, "le la les l' un une des du ce cet cette ces mon ma mes ton ta tes son sa ses notre nos votre vos leurs tout toute tous toutes chaque quelque quelques aucun aucune"),
            (Preposition, None, "de d' à au aux dans par pour avec sans sous sur vers chez entre contre depuis pendant avant après selon malgré"),
            (Conjunction, None, "et ou mais donc or ni car si comme quand lorsque lorsqu' puisque puisqu' parce jusqu'"),
            (Particle, None, "ne n' pas plus très bien aussi encore déjà ici là alors puis"),
            (Adverb, None, "jamais toujours souvent parfois presque peut-être soudain"),
        ],
        Lang::Es => &[
            (Verb, Some("ser"), "ser soy eres es somos sois son era eras éramos erais eran fui fuiste fue fuimos fueron seré será seremos serán sería serían sea seas seamos sean fuera fueran fuese sido siendo"),
            (Verb, Some("estar"), "estar estoy estás está estamos estáis están estaba estaban estuve estuvo estuvieron esté estuviera"),
            (Verb, Some("haber"), "haber he has ha hemos habéis han había habías habíamos habían hubo habrá habría haya hayan hubiera"),
            (Verb, Some("poder"), "poder puedo puedes puede podemos pueden podía podían pudo pudieron podrá podría"),
            (Verb, Some("ir"), "ir voy vas va vamos van iba ibas íbamos iban"),
            (Pronoun, None, "yo tú él ella ello nosotros nosotras vosotros vosotras ellos ellas usted ustedes me te se nos os le les lo que qué quien quién quienes cual cuál cuales cuyo cuya algo alguien nada nadie mí ti sí"),
            (Determiner, None, "el la los las un una unos unas al del este esta esto estos estas ese esa eso esos esas aquel aquella aquellos aquellas mi mis tu tus su sus nuestro nuestra nuestros nuestras vuestro vuestra todo toda todos todas cada otro otra otros otras algún alguna ningún ninguna"),
            (Preposition, None, "a ante bajo con contra de desde en entre hacia hasta para por según sin sobre tras durante"),
            (Conjunction, None, "y e o u pero sino ni porque aunque si cuando como mientras pues"),
            (Particle, None, "no ya muy más menos también tampoco aquí allí ahí así entonces luego"),
            (Adverb, None, "nunca siempre todavía casi pronto tarde ayer hoy mañana"),
        ],
        Lang::It => &[
            (Verb, Some("essere"), "essere sono sei è siamo siete ero eri era eravamo erano fui fu furono sarò sarà saremo saranno sarei sarebbe sarebbero sia siano fosse fossero stato stata stati state"),
            (Verb, Some("avere"), "avere ho hai ha abbiamo avete hanno avevo aveva avevamo avevano ebbi ebbe ebbero avrò avrà avranno avrebbe abbia avuto"),
            (Verb, Some("potere"), "potere posso puoi può possiamo potete possono poteva potevano potrebbe"),
            (Verb, Some("dovere"), "dovere devo devi deve dobbiamo dovete devono doveva dovrebbe"),
            (Pronoun, None, "io tu lui lei egli ella esso essa noi voi essi esse loro mi m' ti t' si s' ci c' vi v' ne n' gli che chi cui quale quali qualcuno qualcosa niente nulla nessuno me te sé"),
            (Determiner, None, "il lo la i le l' un uno una un' questo questa questi queste quest' quello quella quelli quelle quell' quel mio mia miei mie tuo tua tuoi tue suo sua suoi sue nostro nostra nostri nostre vostro vostra tutto tutta tutti tutte ogni"),
            (Preposition, None, "di d' a da in con su per tra fra senza sotto sopra verso dopo del dello della dell' dei degli delle al allo alla all' ai agli alle dal dallo dalla dall' dai dagli dalle nel nello nella nell' nei negli nelle sul sullo sulla sull' sui sugli sulle col coll'"),
            (Conjunction, None, "e ed o od ma però se quando come perché mentre anche oppure"),
            (Particle, None, "non più già molto ancora qui qua lì là così poi ecco"),
            (Adverb, None, "sempre mai spesso quasi subito ieri oggi domani"),
        ],
        Lang::Pt => &[
            (Verb, Some("ser"), "ser sou és é somos são era eras éramos eram fui foi fomos foram serei será seremos serão seria seriam seja sejam fosse fossem sido sendo"),
            (Verb, Some("estar"), "estar estou estás está estamos estão estava estavam esteve estiveram esteja"),
            (Verb, Some("ter"), "ter tenho tens tem temos têm tinha tinham teve tiveram terá teria tido"),
            (Verb, Some("ir"), "ir vou vais vai vamos vão ia iam"),
            (Verb, Some("poder"), "poder posso podes pode podemos podem podia podiam pôde poderá poderia"),
            (Pronoun, None, "eu tu ele ela nós vós eles elas você vocês me te se lhe lhes nos vos mim ti si que quem qual quais cujo cuja algo alguém nada ninguém"),
            (Determiner, None, "o a os as um uma uns umas este esta isto estes estas esse essa isso esses essas aquele aquela aquilo aqueles aquelas meu minha meus minhas teu tua seu sua seus suas nosso nossa nossos nossas todo toda todos todas cada outro outra outros outras"),
            (Preposition, None, "de em por para com sem sob sobre entre até desde contra após durante do da dos das no na nos nas ao aos à às pelo pela pelos pelas num numa dum duma"),
            (Conjunction, None, "e ou mas porém nem porque quando como enquanto embora pois"),
            (Particle, None, "não já muito mais menos também aqui ali lá então assim"),
            (Adverb, None, "nunca sempre ainda quase logo cedo tarde ontem hoje amanhã"),
        ],
    }
}

/// An open-class word: its class from its ending and the words before it, and a base word
/// when the stemmer's would miss.
fn open(w: &str, word: &str, lang: Lang, before: &[Token]) -> (Class, Option<String>) {
    let n = w.chars().count();
    let capital = word.chars().next().is_some_and(char::is_uppercase);
    let after_article = before.last().is_some_and(|t| matches!(t.class, Determiner | Preposition));
    let noun = (Class::Noun, None);
    match lang {
        Lang::En => {
            if w == "'s" {
                // “it's” is “it is”; “Anna's” is whose.
                let be = before.last().is_some_and(|t| t.class == Pronoun);
                return if be { (Verb, Some("be".into())) } else { (Particle, None) };
            }
            if after_article || (capital && !before.is_empty()) {
                return noun;
            }
            let ed = n >= 5 && w.ends_with("ed") && !w.ends_with("eed") && !EN_ED_ADJECTIVES.contains(&w);
            if ed || EN_IRREGULAR.contains(&w) || (n >= 5 && w.ends_with("ing")) {
                return (Verb, None);
            }
            if n >= 5 && w.ends_with("ly") && !EN_LY_NOUNS.contains(&w) {
                return (Adverb, None);
            }
            noun
        }
        Lang::Ru => {
            if capital {
                return noun;
            }
            let after_byt = before.iter().rev().take(2).any(|t| t.lemma.as_deref() == Some("быть"));
            if ru_participle(w, n, after_byt) {
                return (Verb, None);
            }
            noun
        }
        Lang::De => {
            // Nouns are capitalised; a lowercase word may be a participle.
            if capital {
                return noun;
            }
            if let Some(infinitive) = de_participle(w, n) {
                let stem = Stemmer::create(Algorithm::German).stem(&infinitive).into_owned();
                return (Verb, Some(stem));
            }
            (Class::Adjective, None)
        }
        Lang::Es => {
            if after_article || capital {
                return noun;
            }
            let participle = ["ado", "ada", "ados", "adas", "ido", "ida", "idos", "idas"].iter().any(|e| w.ends_with(e));
            let irregular = ES_IRREGULAR.iter().any(|p| w.strip_prefix(p.trim_end_matches('o')).is_some_and(|end| ["o", "a", "os", "as"].contains(&end)));
            if n >= 5 && (participle || irregular) && !ES_NOT_PARTICIPLES.contains(&w) {
                return (Verb, None);
            }
            if w.ends_with("mente") {
                return (Adverb, None);
            }
            noun
        }
        Lang::Fr | Lang::It | Lang::Pt => {
            let adverb = if lang == Lang::Fr { "ment" } else { "mente" };
            if !capital && n > adverb.len() + 2 && w.ends_with(adverb) {
                return (Adverb, None);
            }
            noun
        }
    }
}

/// -ed words that are adjectives after “be”.
const EN_ED_ADJECTIVES: &[&str] = &[
    "tired", "bored", "excited", "interested", "scared", "worried", "surprised", "pleased", "married", "ashamed", "amused", "annoyed", "confused", "convinced", "determined", "disappointed", "embarrassed", "exhausted",
    "frightened", "satisfied", "shocked", "terrified", "relieved", "delighted", "depressed", "naked", "wicked", "sacred", "beloved", "crooked", "wretched", "ragged", "rugged", "learned", "aged", "hundred", "kindred",
];

const EN_LY_NOUNS: &[&str] = &["family", "reply", "supply", "belly", "jelly", "bully", "rally", "holly", "italy", "assembly", "monopoly", "anomaly", "melancholy"];

const ES_NOT_PARTICIPLES: &[&str] = &[
    "nada", "cada", "vida", "lado", "todo", "toda", "medida", "comida", "bebida", "salida", "partida", "herida", "estado", "mercado", "soldado", "cuidado", "abogado", "pecado", "helado", "grado", "ciudad", "verdad", "mitad",
];

/// Prefixes Russian participles take.
const RU_PREFIXES: &[&str] = &["вз", "вс", "вы", "до", "за", "из", "ис", "на", "над", "недо", "об", "обо", "от", "пере", "по", "под", "пред", "при", "про", "раз", "рас", "в", "о", "с", "со", "у"];
/// Stems of nouns and short adjectives shaped like participles.
const RU_NOT_PARTICIPLES: &[&str] = &[
    "стен", "стран", "сторон", "сцен", "смен", "измен", "забот", "работ", "охот", "суббот", "высот", "стакан", "обман", "океан", "уверен", "соглас", "свобод", "довол", "способ", "споко", "вопрос",
];

/// A short passive participle (написано, открыты, взят) — after *быть* any of its forms, else
/// only the long neuter and plural ones, which nouns rarely share.
fn ru_participle(w: &str, n: usize, after_byt: bool) -> bool {
    if !RU_PREFIXES.iter().any(|p| w.starts_with(p)) || RU_NOT_PARTICIPLES.iter().any(|s| w.starts_with(s)) {
        return false;
    }
    let (en, t) = (["ан", "ян", "ен", "ён"], ["ыт", "ит", "ут", "ят", "от", "ёрт", "ерт"]);
    let ends = |stems: &[&str], vowels: &[&str]| stems.iter().any(|s| vowels.iter().any(|v| w.ends_with(&format!("{s}{v}"))));
    if ends(&en, &["о", "ы"]) || ends(&t, &["о", "ы"]) {
        return after_byt || n >= 7;
    }
    after_byt && (ends(&en, &["а"]) || ends(&t, &["а"]) || (n >= 6 && ends(&en, &[""])) || (n >= 4 && ends(&t, &[""])))
}

/// Separable prefixes a Partizip II puts before its ge-.
const DE_SEPARABLE: &[&str] = &["ab", "an", "auf", "aus", "bei", "dar", "durch", "ein", "fest", "her", "hin", "los", "mit", "nach", "um", "vor", "vorbei", "weg", "weiter", "zu", "zurück", "zusammen"];
const DE_INSEPARABLE: &[&str] = &["be", "ver", "er", "ent", "zer", "emp", "miss"];
/// Infinitives and adjectives shaped like a Partizip II.
const DE_NOT_PARTICIPLES: &[&str] = &[
    "gehen", "geben", "gelten", "gewinnen", "genießen", "geschehen", "gehören", "gelingen", "gestehen", "gewähren", "gewöhnen", "gebrauchen", "gefallen", "gesamt", "gerecht", "bereit", "bereits", "bestimmt", "berühmt",
    "bewusst", "ernst", "verrückt", "beliebt", "entfernt", "erst", "gegen", "genug", "gestern",
];

/// A Partizip II (gebaut, aufgebaut, verkauft, gegeben) and its infinitive.
fn de_participle(w: &str, n: usize) -> Option<String> {
    if n < 5 || DE_NOT_PARTICIPLES.contains(&w) {
        return None;
    }
    let (sep, rest) = DE_SEPARABLE.iter().find_map(|p| w.strip_prefix(p).filter(|r| r.starts_with("ge")).map(|r| (*p, r))).unwrap_or(("", w));
    let infinitive = |stem: &str| if stem.ends_with("en") { stem.to_string() } else { format!("{}en", stem.strip_suffix("et").or_else(|| stem.strip_suffix('t')).unwrap_or(stem)) };
    if let Some(stem) = rest.strip_prefix("ge").filter(|s| s.chars().count() >= 3 && (s.ends_with('t') || s.ends_with("en"))) {
        return Some(format!("{sep}{}", infinitive(stem)));
    }
    let inseparable = DE_INSEPARABLE.iter().any(|p| w.starts_with(p)) && w.ends_with('t');
    inseparable.then(|| infinitive(w))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{echoes, is_passive};

    fn words(text: &str, lang: Lang) -> Vec<String> {
        tag(text, lang).into_iter().map(|t| t.word).collect()
    }

    fn passive(text: &str, lang: Lang) -> bool {
        is_passive(&tag(text, lang), lang)
    }

    #[test]
    fn splits_as_the_mac_does() {
        assert_eq!(words("You can’t, isn't it? It's Anna’s.", Lang::En), ["You", "ca", "n’t", "is", "n't", "it", "It", "'s", "Anna", "’s"]);
        assert_eq!(words("l’écriture qu'il aujourd'hui — 12 -", Lang::Fr), ["l’", "écriture", "qu'", "il", "aujourd'hui", "12"]);
        assert_eq!(words("dell'anima, l'altro", Lang::It), ["dell'", "anima", "l'", "altro"]);
        let t = tag("It's late. Anna's book.", Lang::En);
        assert_eq!((t[1].class, t[1].base()), (Class::Verb, "be".to_string()));
        assert_eq!(t[4].class, Class::Particle);
    }

    #[test]
    fn english() {
        assert!(passive("Your sentence isn’t erased by a good editor.", Lang::En));
        assert!(passive("It was never written.", Lang::En));
        assert!(passive("The house has been sold.", Lang::En));
        assert!(passive("They're invited.", Lang::En));
        assert!(!passive("Editors have always known this.", Lang::En));
        assert!(!passive("They write a suggestion in the margin and let you decide, and the page holds both versions until you are ready.", Lang::En));
        assert!(!passive("I was tired.", Lang::En));
        assert!(!passive("She was walking home.", Lang::En));
        assert!(!passive("A good editor erased it.", Lang::En));
        assert!(!passive("It was a red car.", Lang::En));
    }

    #[test]
    fn russian() {
        assert!(passive("Письмо было написано ещё вчера.", Lang::Ru));
        assert!(passive("Окно открыто настежь.", Lang::Ru));
        assert!(passive("Он был взят в плен.", Lang::Ru));
        assert!(passive("Работа была сделана.", Lang::Ru));
        assert!(!passive("Они видят море.", Lang::Ru));
        assert!(!passive("Мы долго шли молча и не говорили ни слова.", Lang::Ru));
        assert!(!passive("Я держал его в кармане и не знал, что с ним делать.", Lang::Ru));
        assert!(!passive("Это была страна туманов.", Lang::Ru));
        assert!(!passive("Он был уверен, что было рано.", Lang::Ru));
        assert!(!passive("Работы было много.", Lang::Ru));
    }

    #[test]
    fn german() {
        assert!(passive("Das Haus wurde gestern verkauft.", Lang::De));
        assert!(passive("Es wird gebaut.", Lang::De));
        assert!(passive("Die Brücke wurde wieder aufgebaut.", Lang::De));
        assert!(passive("Das Buch wurde ihm gegeben.", Lang::De));
        assert!(!passive("Er wird morgen gehen.", Lang::De));
        assert!(!passive("Er wird bereit sein.", Lang::De));
        assert!(!passive("Er hat das Haus verkauft.", Lang::De));
        let t = tag("Es wird gebaut", Lang::De);
        assert_eq!(t[2].base(), tag("bauen", Lang::De)[0].base(), "the participle's base is its verb's");
    }

    #[test]
    fn spanish() {
        assert!(passive("La carta fue escrita ayer.", Lang::Es));
        assert!(passive("Las casas fueron vendidas.", Lang::Es));
        assert!(passive("El libro fue abierto.", Lang::Es));
        assert!(!passive("Es importante.", Lang::Es));
        assert!(!passive("No es nada.", Lang::Es));
        assert!(!passive("Esa fue la vida que tuvo.", Lang::Es));
    }

    #[test]
    fn echoes_by_base_word() {
        let text = "Most of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear. A week later you can’t remember those words, only that one of them was better.";
        let e = echoes(&tag(text, Lang::En), Lang::En, &HashSet::new());
        assert_eq!(e.iter().map(|e| (e.word.as_str(), e.between)).collect::<Vec<_>>(), [("words", 18)], "counted as the Mac counts");
        let e = echoes(&tag("You keep them close, and keeping them is work.", Lang::En), Lang::En, &HashSet::new());
        assert_eq!(e.iter().map(|e| e.word.as_str()).collect::<Vec<_>>(), ["keeping"]);
        let e = echoes(&tag("Мы вернулись домой поздно. Дома было тихо, и в доме пахло хлебом.", Lang::Ru), Lang::Ru, &HashSet::new());
        assert_eq!(e.iter().map(|e| e.word.as_str()).collect::<Vec<_>>(), ["доме"], "Дома is capitalised, as a name would be");
        let e = echoes(&tag("Il est là, et il sera là pour elle et pour nous, avec nous.", Lang::Fr), Lang::Fr, &HashSet::new());
        assert!(e.is_empty(), "function words and common adverbs: {e:?}");
    }
}
