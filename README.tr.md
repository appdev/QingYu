<p align="center">
<img alt="QingYu" src="logo.png" width="128">
<br>
<strong>QingYu</strong>
<br>
<em>QingYu · Aydınlık pencereler, sakin bir masa, usulca söylenen sözler.</em>
<br><br>
Sessiz, berrak ve denetimi sizde olan bir yazma alanı.
</p>

<p align="center">
<a href="README.md">English</a>
| <a href="README.zh-CN.md">中文</a>
| <a href="README.ja.md">日本語</a>
| <b>Türkçe</b>
</p>

> QingYu, açık kaynaklı [SiYuan](https://github.com/siyuan-note/siyuan) projesi temel alınarak geliştirilmiştir ve [AGPL-3.0](LICENSE) lisansına tabidir. QingYu resmî bir SiYuan dağıtımı değildir; ürün tasarımı, özellik tercihleri, sürümler ve destek QingYu projesi tarafından bağımsız olarak yürütülür.

## QingYu neden var

Notlar, yönetmeniz gereken yeni bir yüke dönüşmemeli.

QingYu arayüzü, yapıyı ve araçları olması gereken yerde tutarak dikkatinizi yeniden kelimelere verir. Bir cümleyle başlayabilir, düşünceler büyüdükçe aralarında bağ kurabilir, kaynakları düzenleyebilir ve zaman içinde kalıcı bilgi oluşturabilirsiniz. Önceden kusursuz bir yöntem tasarlamanız ya da bilginizi bir hesap sistemine teslim etmeniz gerekmez.

QingYu daha fazla özellik toplama yarışına girmez. Yazmanın doğal hissettirmesine, içeriğin anlaşılır kalmasına ve bilginizin size ait olmaya devam etmesine önem verir.

## Temel deneyim

### Gürültü olmadan yazın

Blok tabanlı düzenleyici, serbest yazımı görünür bir yapıyla birleştirir. Markdown WYSIWYG, ana hatlar, matematik, diyagramlar ve büyük belgeler gerektiğinde hazırdır; gerekmediğinde kelimelerin önüne geçmez.

### Düşünceler yeniden karşılaşsın

Blok referansları, geri bağlantılar, sanal referanslar ve tam metin arama bağlantıların doğal biçimde ortaya çıkmasına yardımcı olur. Yazmaya başlamadan önce kusursuz bir sınıflandırma kurmanız gerekmez; eski düşünceler yeniden anlam kazandığında geri dönebilir.

### Kaynakları bağlamında tutun

Tablo veritabanları, PDF okuma ve açıklama, web kırpma, OCR, varlık dosyaları ile esnek içe ve dışa aktarma seçenekleri, topladığınız malzemeyi düşünmenin ve yazmanın parçası hâline getirir.

### Kendi çalışma alanınızı şekillendirin

Belge ağaçları, etiketler, yer imleri, şablonlar, kod parçacıkları, temalar, simgeler ve eklentiler çalışma alanını uyarlamanıza izin verir. QingYu sağlam bir temel sunar, ancak tek doğru not alma yöntemini dayatmaz.

### Gerektiğinde daha ileri gidin

Yerel API, yerleşik MCP Server, komut satırı araçları ve kendi sunucunuzda çalıştırma seçenekleri otomasyon ve genişletme için kapı bırakır. Bu yetenekler günlük yazma deneyiminin merkezinde değil, arka planında durur.

## Verileriniz, alanınız

QingYu içeriği seçtiğiniz yerel çalışma alanında saklar ve veri sınırlarını anlaşılır, taşınabilir ve kurtarılabilir tutmayı amaçlar.

- Şifreli not defterleri hassas içerik için ayrı koruma sağlar.
- Yerel depo anlık görüntüleri, geçmiş ve kurtarma uzun vadeli çalışmaları korumaya yardımcı olur.
- S3, WebDAV ve yerel dosya sistemi eşitlemesiyle depolama sağlayıcısını siz seçer ve yönetirsiniz.
- Temel özellikler için QingYu bulut hesabı veya resmî bulut eşitleme hizmeti gerekmez.
- Kullanım davranışı, tanılama verileri, kurulum olayları, cihaz tanımlayıcıları ya da benzer telemetri verileri etkin biçimde gönderilmez.
- Markdown, PDF, Word, HTML ve diğer dışa aktarma yolları içeriğin tek bir arayüzde kilitli kalmasını önler.

Gizlilik yalnızca bir slogan değil; hesaplar, ağ, depolama ve ürün kararları için sürekli bir kısıttır.

## Kimler için

- Not alma sistemini sürekli yeniden kurmak yerine yıllarca yazmak isteyenler.
- Kaynakları, literatürü, projeleri ve gelişen düşünceleri düzenleyen araştırmacılar.
- Yerel veriye, açık biçimlere, yedeklemeye ve taşıma özgürlüğüne değer veren bilgi çalışanları.
- Görünür yapı isteyen fakat araçların düşünceyi bölmesini istemeyen yazarlar.
- Çalışma alanını eklentiler, otomasyon veya kendi sunucusuyla genişletmek isteyen kullanıcılar.

## Proje durumu

QingYu etkin olarak geliştirilmektedir; ürün sınırları, uyumluluk ve sürüm süreci henüz istikrara kavuşmaktadır. Resmî dağıtım kanalları hazırlanmaktadır. SiYuan'ın resmî kurulum paketleri, uygulama mağazası sürümleri ve bulut hizmetleri QingYu sürümleri veya hizmetleri değildir.

Bu depo şu anda geliştirme, ürün yönünü inceleme ve kaynak koddan derleme amacıyla kullanılmaktadır. Kaydedilen değişiklikler için [değişiklik günlüğüne](CHANGELOG.md) bakabilirsiniz.

## Geliştiriciler için

QingYu bir Go çekirdeğini TypeScript ön yüzüyle birleştirir; ancak bu README bilinçli olarak ürün düzeyinde kalır. Uygulama ayrıntıları için şu kaynaklardan başlayabilirsiniz:

- [API belgeleri](docs/API.md)
- [Katkı rehberi](.github/CONTRIBUTING.md)
- [Değişiklik günlüğü](CHANGELOG.md)
- [Ürün kimliği tasarımı](docs/superpowers/specs/2026-08-10-qingyu-product-identity-design.md)
- [Özellik sınırı tasarımı](docs/superpowers/specs/2026-08-10-feature-removal-design.md)
- `scripts/` altındaki macOS, Linux ve Windows derleme girişleri

Gerekli araç sürümleri için `kernel/go.mod`, `app/package.json` ve proje iş akışlarını temel alın.

## SiYuan üzerine inşa edildi

QingYu, SiYuan'ın olgun blok düzenleyicisini, veri biçimini ve açık kaynak ekosistemini temel alırken ürün kimliğini, özellik sınırlarını ve günlük deneyimi yeniden şekillendirir.

Gerekli veri ve eklenti uyumluluğunu korur; ancak uygulama kimlikleri, yapılandırma dizinleri, protokoller, bağlantı noktaları, çekirdek adlandırması ve ürün kararları bağımsızdır. QingYu resmî bir SiYuan dağıtımı değildir ve SiYuan ekibini temsil etmez. QingYu ile ilgili sorunlar, derlemeler, sürümler ve destek QingYu projesinin sorumluluğundadır.

Bu temeli mümkün kılan SiYuan ekibine, Lute'a, diğer üst kaynak projelere ve tüm açık kaynak katkıcılarına teşekkür ederiz. Üst kaynak proje: [github.com/siyuan-note/siyuan](https://github.com/siyuan-note/siyuan).

## Açık kaynak ve teşekkürler

QingYu [GNU Affero General Public License v3.0](LICENSE) ile dağıtılır. Dağıtımlar ve değişiklikler lisansa uymalı, özgün projenin ve katkıcılarının telif ve atıf bilgilerini korumalıdır.

Her not biraz daha hafif, her düşünce biraz daha berrak olsun.
