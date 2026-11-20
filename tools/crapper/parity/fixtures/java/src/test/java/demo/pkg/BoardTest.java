package demo.pkg;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class BoardTest {
    @Test
    void placeRejectsNegativeInput() {
        assertEquals(-1, new Board().place(-1, 0));
    }

    @Test
    void placeCountsSteps() {
        assertEquals(10, new Board().place(0, 0));
    }

    @Test
    void namesTheNumber() {
        assertEquals("one", new Board().name(1));
        assertEquals("two", new Board().name(2));
        assertEquals("many", new Board().name(3));
    }
}
