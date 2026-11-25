use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

#[test]
fn protocol_encoding_matches_the_dependency_for_default_cells() {
    let mut native = NativePicker::halfblocks();

    native.set_protocol_type(ProtocolType::Iterm2);
    let picker = Picker::new(FontSize::new(10, 20), ProtocolType::Iterm2);
    let available = Size::new(8, 5);

    for (width, height) in [(10, 20), (31, 43), (173, 201)] {
        let image = DynamicImage::new_rgba8(width, height);
        let expected = native
            .new_protocol(image.clone(), available, Resize::Fit(None))
            .unwrap();
        let actual = picker.new_protocol(image, available, &(Resize::Fit(None))).unwrap();

        assert_eq!(actual.size(), expected.size());
        assert_eq!(render(&actual, available), render(&expected, available));
    }
}

#[test]
fn custom_cell_size_stays_in_the_image_layout() {
    let picker = Picker::new(FontSize::new(13, 27), ProtocolType::Iterm2);
    let image = DynamicImage::new_rgba8(26, 54);
    let protocol = picker
        .new_protocol(image, Size::new(8, 8), &(Resize::Fit(None)))
        .unwrap();

    assert_eq!((picker.font_size().width, picker.font_size().height), (13, 27));
    assert_eq!(protocol.size(), Size::new(2, 2));
}

fn render(protocol: &Protocol, size: Size) -> Buffer {
    let area = Rect::new(0, 0, size.width, size.height);
    let mut buffer = Buffer::empty(area);

    ratatui_image::Image::new(protocol).render(area, &mut buffer);
    buffer
}
